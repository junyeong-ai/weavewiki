# 03. 품질 시스템 및 검증 체계 심층 분석

> 100% 코드 기반 사실 분석 - 추측 배제

---

## 1. 전체 품질 시스템 아키텍처

### 1.1 검증 계층 구조 (코드에서 확인된 실제 구조)

claudegen의 품질 시스템은 5개 층위의 검증 파이프라인으로 구성된다:

```
Layer 0: Artifact Validators (src/pipeline/generation/artifact/validators.rs)
         - 구조적 유효성 (이름 존재, description 존재)
         - 파일 참조 환각 탐지 (VerifiedFileRegistry 대조)

Layer 1: Validity Filter (src/pipeline/validation/simplified.rs:ValidityFilterResult)
         - 아티팩트별 validity 상태 확인 (Valid/Hallucinated)
         - 개별 아티팩트 기반 필터링 (전체 배치 실패 아닌 개별 제거)

Layer 2: Consistency Check (src/pipeline/validation/simplified.rs:ConsistencyResult)
         - 중복 이름 검출 (skill, agent, rule)
         - Agent->Skill 참조 무결성

Layer 3: Cross-Artifact Validation (src/pipeline/validation/simplified.rs:CrossValidationResult)
         - Evidence Traceability: 파일 참조 유효성 검증
         - Plan Consistency: overview 존재, 최소 아티팩트 생성 여부

Layer 4: LLM Judge (src/pipeline/quality/judge.rs:LlmJudge)
         - 의미적 품질 평가 (actionability, specificity, evidence quality)
         - Value Assessment (domain_specificity, information_density 등)
         - 환각 자기보고 + programmatic override

Layer 5: Deep Review (src/pipeline/deep_review.rs:DeepReviewEngine)
         - Two-pass 리뷰 (Pass 1: 전체 감사, Pass 2: 회귀 검사)
         - Programmatic: 파일 참조, 포맷 검증
         - LLM: 의미적 품질, 교차 아티팩트 일관성
```

### 1.2 검증 흐름 (quality_loop.rs에서 확인)

```
QualityLoop::run()
  -> 분석 신뢰도 체크 (analysis_confidence < target_quality)
  -> 합성 신뢰도 체크 (synthesis_confidence < target_quality)
  -> 에비던스 검증 (validate_evidence: invalid_ratio > threshold)
  -> 품질 점수 체크 (quality_score >= min_quality)
  -> Deep Review (two-pass)
  -> Validation Pipeline (3-layer)
  -> Clean Pass Status (연속 2회 통과 필요)
```

---

## 2. Programmatic 검증 상세 분석

### 2.1 파일 참조 검증 (100% 결정론적)

**핵심 함수**: `file_reference.rs:is_valid_file_ref()`

```rust
pub fn is_valid_file_ref(path: &str, line_start: Option<u32>, registry: &VerifiedFileRegistry) -> bool {
    if !registry.contains(path) { return false; }
    if let Some(line) = line_start && line > 0
        && let Some(max_lines) = registry.line_count(path) {
        return (line as usize) <= max_lines;
    }
    true
}
```

**검증 범위**:
- 파일 존재 여부: `registry.contains(path)` -- 확실한 사실 검증
- 라인 범위 유효성: `line <= max_lines` -- 확실한 사실 검증
- `@path:line` 형식의 참조만 검증

**검증 불가 영역** (코드에서 확인됨):
- 참조된 라인의 **내용**이 주장과 일치하는지 여부 -- 검증하지 않음
- 파일은 존재하지만 참조가 **의미적으로 관련 있는지** -- 검증하지 않음
- `@path` 형식(라인 번호 없는) 참조의 의미적 정확성 -- 라인 검증 불가

### 2.2 환각 탐지 메커니즘

**judge.rs:parse_response()** (line 324-348):

```rust
// 1. LLM 자기보고 환각 = 항상 신뢰
let validity = if output.hallucinated {
    ValidityState::Hallucinated
} else {
    // 2. Programmatic override: invalid_ref_threshold(기본 0.4) 초과시 환각 판정
    let (total_refs, invalid_refs) = self.verify_references(artifact_content);
    if total_refs > 0 {
        let invalid_ratio = invalid_refs as f32 / total_refs as f32;
        if invalid_ratio > self.invalid_ref_threshold {
            ValidityState::Hallucinated
        } else { ValidityState::Valid }
    } else { ValidityState::Valid }
};
```

**핵심 설계 결정**:
- LLM이 `hallucinated=true`로 보고하면 **무조건 신뢰** (테스트 `test_llm_reported_hallucination_always_trusted`로 검증됨)
- LLM이 `hallucinated=false`로 보고해도, invalid 비율이 threshold(기본 40%) 초과시 **programmatic override**
- 참조가 0개인 콘텐츠는 자동으로 `Valid` -- **이것은 맹점**: 참조 없이 거짓 주장을 하는 콘텐츠는 탐지 불가

### 2.3 ArtifactQuality 게이트 (types/quality.rs)

```rust
const REFERENCE_VALIDITY_GATE: f32 = 0.90;

pub fn is_acceptable(&self) -> bool {
    self.reference_validity >= REFERENCE_VALIDITY_GATE  // 90%+ 유효 참조 필요
}
```

- `reference_validity`만으로 accept/reject 결정 (line 75-77)
- `reference_density`는 진단 목적만 (line 69-71 주석: "NOT used for gating")
- 참조가 0개면 `reference_validity = 1.0` (line 109-112) -- 참조 없는 콘텐츠는 항상 통과

### 2.4 Validity Filter (simplified.rs)

```rust
pub fn check(skills, agents, rules) -> ValidityFilterResult {
    // 개별 아티팩트의 quality.validity 상태 확인
    // passed = valid_count > 0 (하나라도 유효하면 통과)
}
```

**주목할 점**: `passed`는 `valid > 0`으로 판정 (line 74). 즉 50개 중 1개만 유효해도 "passed"가 true. 단, `hallucinated_count`와 `validity_ratio`는 별도 제공되어 상위 레이어에서 활용.

### 2.5 Consistency Check (simplified.rs)

**검증 범위**:
- 중복 이름 검출 (HashSet 기반) -- 100% 결정론적
- Agent -> Skill 참조 유효성 -- 100% 결정론적
- 모노레포 시 rules 없는 경우 허용 (line 139-141)

**검증 불가**:
- 의미적 중복 (이름은 다르지만 내용이 동일)
- 불필요한 아티팩트 검출

### 2.6 Format Validation (deep_review.rs:validate_format)

```rust
fn validate_format(&self, artifacts: &ReviewArtifacts) -> CheckResult {
    // skills/agents: starts_with("---") 및 contains("name:")
    // rules: starts_with("---") 시 contains("paths:") 필요
}
```

**코드 내 자체 인정된 제한** (line 482-490):
```
// Note: This is a quick syntactic check, not full YAML parsing.
// Limitations:
// - `starts_with("---")` may match non-YAML content (e.g., markdown horizontal rules)
// - `contains("name:")` doesn't verify field is in frontmatter section
// - Doesn't validate YAML syntax or required field completeness
```

---

## 3. LLM 기반 검증 상세 분석

### 3.1 LlmJudge (judge.rs)

**프롬프트 구조** (line 237-286):
- Hallucination Detection: 파일/라인 참조 정확성, 함수/클래스 참조 정확성
- Actionability: 명확하고 구체적인 액션 여부
- Evidence: 유효한 `@file:line` 참조 존재 여부
- Value Assessment: domain_specificity, actionability, information_density, completeness, generic_content_ratio

**응답 파싱 전략**:
1. 정상 JSON 파싱 시도 (`serde_json::from_str`)
2. 실패시 `deserialize_llm_response` 시도
3. 실패시 partial recovery (`try_recover_partial`): quality_score, hallucinated, issues 개별 추출
4. JSON 전체 파싱 실패시 embedded JSON 추출 (`try_recover_from_raw`: 첫 `{`~마지막 `}`)
5. Recovery시 `MIN_RECOVERY_QUALITY = 0.1` 이상이어야 유효

**Aggregation** (line 131-201):
- `overall_score`: 개별 결과의 산술 평균
- `validity`: 하나라도 Hallucinated면 전체 Hallucinated
- `issues`, `suggestions`: 모든 결과에서 flat_map 수집
- `value_assessment`: 각 차원의 산술 평균

### 3.2 Deep Review Engine (deep_review.rs)

**Two-Pass 구조**:

```
execute_two_pass_review():
  while consecutive_passes < required_passes && total_attempts < max_attempts:
    execute_single_pass():
      - validate_evidence() [programmatic: 파일 참조 + 라인 범위]
      - validate_format() [programmatic: YAML frontmatter]
      - check_semantic_quality() [LLM: 의미적 품질]
      - check_cross_artifact_consistency() [LLM: 교차 일관성]

    if passed && consecutive > 0 && check_regression:
      regression check (새 이슈 발생 여부)

    if passed: consecutive++
    else: consecutive = 0, baseline = None
```

**Regression Check** (line 827-865):
- baseline과 current 이슈를 (artifact, message) 튜플의 HashSet으로 비교
- 새 이슈가 있으면 `has_regression = true`
- 한계: 문자열 완전 일치 비교이므로 동일 이슈의 미세한 문구 변경도 "새 이슈"로 판정

**Semantic Quality 프롬프트** (line 574-656):
- Actionability (0-100): 구체적이고 실행 가능한 지침인가
- Specificity (0-100): 프로젝트 고유 지식을 포함하는가
- Value-Add (0-100): Claude의 기존 지식 이상의 가치를 제공하는가
- Evidence Quality (0-100): 파일 참조가 의미 있는가

### 3.3 Evidence Label Scanner (evidence_scanner.rs)

**태그 기반 스캔**:
```rust
static VERIFIED_RE: Regex = r"\[Verified(?::|\s)[^\]]*\]"
static INFERRED_RE: Regex = r"\[Inferred(?::|\s)[^\]]*\]"
static CONVENTION_RE: Regex = r"\[Convention(?:\]|(?::|\s)[^\]]*\])"
```

- `scan_and_validate()`: `[Verified:...]` 태그 내부의 파일 참조를 registry와 대조
- `ValidatedEvidenceProfile`: verified_valid, verified_invalid 카운트 제공
- verification_ratio: `verified_valid / (verified_valid + verified_invalid)`

---

## 4. 리파인먼트 루프 분석

### 4.1 RefinementEngine (refinement/engine.rs)

**핵심 루프**:
```
loop {
    assess_quality(skills, agents, rules, claude_md)  // 또는 assess_quality_selective
    check_convergence(metrics, combined_quality, cfg, state)
    if converged -> return success

    prune_hallucinated_artifacts()  // 환각 아티팩트 제거
    identify_all_issues()  // LlmJudge 이슈 -> DetectedIssue 변환
    apply_refinements()  // 전략 기반 개선
    handle_quality_patterns()  // 정체/진동 감지
}
```

### 4.2 수렴 보장 메커니즘 (무한 루프 방지)

**1차 안전장치: max_iterations** (engine.rs line 436):
```rust
if iteration >= cfg.max_iterations { break; }
```

**2차 안전장치: 수렴 판정** (check_convergence):
- `QualityLevel::AtTarget` + `consecutive_clean_passes >= required` -> 수렴
- `QualityLevel::AtFloor` + `consecutive_clean_passes >= required * FLOOR_CONVERGENCE_PASS_MULTIPLIER` -> 수렴

**3차 안전장치: 진동 탐지** (detect_level_oscillation):
- level_history에서 최근 window 크기만큼의 변화 비율이 OSCILLATION_DETECTION_THRESHOLD 이상이면 진동
- 진동 + quality >= floor -> 현재 상태 수락

**4차 안전장치: 정체 감지** (handle_quality_patterns):
- stagnation_count가 patience 초과시 strategy escalation
- 진동 + 정체 동시 발생시 force_regeneration

**5차 안전장치: best state 보존**:
- 매 iteration마다 best_quality보다 높으면 snapshot 저장
- max_iterations 도달시 best state 반환

### 4.3 Selective Assessment 최적화

```rust
async fn assess_quality_selective() {
    // 수정된 아티팩트만 re-evaluate (needs_evaluation 체크)
    // 미수정 아티팩트는 cached_judgments 재사용
    // ~85% LLM 호출 절감 (주석 기재)
}
```

**주의**: cache invalidation은 `state.modified_this_iteration`과 `state.cached_judgments` 키 존재 여부로 판단. 외부 요인(파일 변경 등)에 의한 무효화는 처리되지 않음.

### 4.4 Strategy Rotation (strategy/mod.rs)

**두 가지 전략**:
1. `SemanticStrategy`: 의미적 개선 (LLM 기반 body 재작성)
2. `EvidenceStrategy`: 참조 추가 (LLM에 파일 목록 제공 후 참조 보강)

**StrategyRotator 로직**:
- 이슈 유형별 applicable 전략 필터링
- 최근 실패 기록 확인 (3회 연속 실패시 건너뛰기)
- 역사적 성공률 기반 최적 전략 선택
- escalation_level에 따른 전략 순환
- force_regeneration: 가장 공격적인 전략 강제 사용

### 4.5 Quality Assessment (quality_assessment.rs)

**QualityAssessor 수렴 경로** (check 메서드, line 318-368):
```
PATH 1: EarlyExit (early_exit_bypasses_dimensions=true && quality >= threshold)
PATH 2: QualityFloorMet (quality >= floor && minimum_viable dimensions)
PATH 2.5: Relaxed QualityFloor (quality >= floor && any_passed && 3+ dimensions)
PATH 3: Full Quality (core_passed && target met / all dimensions)
```

**check_with_thinking** (line 370-441):
- uncertainty 기반 종료 판단 추가
- 고 uncertainty시 continue (품질 개선 중일 때만)
- 정체 + acceptable quality -> 종료

---

## 5. Outer Quality Loop (quality_loop.rs)

### 5.1 다중 계층 게이트

```
Gate 1: Analysis Confidence (analysis_confidence < target_quality -> escalate depth)
Gate 2: Synthesis Confidence (synthesis_confidence < target_quality -> escalate depth)
Gate 3: Evidence Validation (invalid_ratio > threshold -> escalate or return best)
Gate 4: Quality Score (quality_score >= min_quality -> proceed to deep review)
Gate 5: Deep Review (two-pass LLM review)
Gate 6: Validation Pipeline (validity + consistency + cross-artifact)
Gate 7: Clean Pass (deep_review_passed && validation_passed, 연속 2회 필요)
```

### 5.2 Depth Escalation

```rust
fn try_escalate_analysis_depth(config) -> Option<Config> {
    Fast -> Standard -> Complete -> None (최대 도달)
    max_file_samples *= 1.5
    deep_analysis.max_iterations += 1
}
```

### 5.3 Budget Extension (iteration_state.rs)

- `IterationState`: 동적 iteration 예산 관리
- `MAX_ITERATION_EXTENSION = 2`: 최대 2회 추가 가능
- `QUALITY_IMPROVEMENT_DELTA = 0.02`: 2% 이상 개선시 연장 고려
- `maybe_extend(BudgetExtensionTrigger::QualityImproving)`: 품질 개선 중이면 연장

---

## 6. 피드백 집계 시스템 (feedback.rs)

### 6.1 FeedbackAggregator

**가중치 구조** (line 14-17):
```
Quality:    40% (LlmJudge 점수)
Structural: 30% (모듈 커버리지)
Evidence:   30% (파일 참조 유효성)
```

**수렴 조건** (line 144-145):
```rust
let converged = overall_score >= self.target_quality    // 기본 0.85
    && dimension_scores.all_pass(self.dimension_pass_threshold);  // 기본 0.6
```

### 6.2 Issue Prioritization

```rust
// Impact scores: Critical=1.0, High=0.8, Medium=0.5, Low=0.2
// 정렬: priority DESC, impact_score DESC
```

---

## 7. 에비던스 기반 검증의 강점과 맹점

### 7.1 강점 (코드에서 확인)

1. **파일 존재 검증이 100% 결정론적**: `VerifiedFileRegistry`는 실제 파일 시스템 스캔 결과. `is_valid_file_ref()`는 파일 존재 + 라인 범위를 팩트로 검증.

2. **Programmatic Override가 LLM 환각 보완**: LLM이 "환각 아님"이라 해도 40% 이상 참조가 유효하지 않으면 환각 판정 (judge.rs line 328-348). 테스트 케이스로 검증됨.

3. **개별 아티팩트 필터링**: 전체 배치를 실패시키지 않고 개별 환각 아티팩트만 제거 (ValidityFilterResult, prune_hallucinated_artifacts).

4. **다중 계층 방어**: 5개 층위의 검증이 각각 다른 관점에서 품질 확인.

### 7.2 맹점 (코드에서 확인된 구체적 시나리오)

#### 맹점 1: 참조 없는 허위 주장 탐지 불가

**코드 근거**: `types/quality.rs` line 109-112:
```rust
let reference_validity = if total_refs > 0 {
    valid_refs as f32 / total_refs as f32
} else {
    1.0  // 참조 0개 = validity 1.0
};
```

**시나리오**: "이 프로젝트는 Redis를 캐시 레이어로 사용합니다" 같은 주장이 참조 없이 포함되면, 사실이 아니더라도 programmatic 검증을 통과함. LLM Judge에만 의존.

#### 맹점 2: 파일은 존재하지만 내용이 무관한 참조

**코드 근거**: `file_reference.rs` -- path 존재와 line 범위만 확인:
```rust
pub fn is_valid_file_ref(path, line_start, registry) -> bool {
    if !registry.contains(path) { return false; }
    if let Some(line) = line_start && line > 0 && let Some(max_lines) = registry.line_count(path) {
        return (line as usize) <= max_lines;
    }
    true
}
```

**시나리오**: `@src/main.rs:42`를 참조하면서 "이 파일은 인증 로직을 담당합니다"라고 주장하지만, 실제 line 42는 import문일 수 있음. Programmatic으로는 탐지 불가.

#### 맹점 3: 임계값 의존성

**코드 근거**: `judge.rs` line 334:
```rust
if invalid_ratio > self.invalid_ref_threshold {  // 기본 0.4
```

**시나리오**: 10개 참조 중 4개가 유효하지 않아도(40%) 통과. 3개 유효 + 7개 무효(70% invalid)여야 환각 판정. 이는 상당수의 잘못된 참조를 허용할 수 있음.

#### 맹점 4: Format Validation의 표면적 검사

**코드 근거**: `deep_review.rs` line 491-549, 자체 주석 (line 482-490):
```
// - `starts_with("---")` may match non-YAML content (e.g., markdown horizontal rules)
// - `contains("name:")` doesn't verify field is in frontmatter section
```

**시나리오**: `---\nsome random content with name: somewhere\n---`가 유효한 YAML frontmatter로 오판.

#### 맹점 5: Regression Check의 문자열 의존

**코드 근거**: `deep_review.rs` line 832-835:
```rust
let baseline_issues: HashSet<_> = baseline
    .as_ref()
    .map(|b| b.iter().map(|i| (&i.artifact, &i.message)).collect())
    .unwrap_or_default();
```

**시나리오**: LLM이 동일한 문제를 약간 다른 문구로 보고하면 (예: "Missing reference" vs "Reference missing"), 같은 이슈가 해결된 것이 아님에도 새로운 이슈로 분류됨.

#### 맹점 6: Quick Quality의 구조 편향

**코드 근거**: `strategy/mod.rs` line 489-503:
```rust
pub fn calculate_quick_quality(content: &str) -> f32 {
    let ref_score = (ref_count as f32 / REF_SATURATION_COUNT).min(1.0);
    let content_score = (char_count as f32 / CONTENT_SATURATION_CHARS).min(1.0);
    REF_WEIGHT * ref_score + CONTENT_WEIGHT * content_score
}
```

**시나리오**: 참조가 많고 길기만 한 저품질 콘텐츠가 높은 quick_quality를 받을 수 있음. 이 점수는 refinement acceptance에 사용됨(semantic/evidence strategy에서 quality retention 체크).

---

## 8. LLM 판단 기반 검증의 효과성

### 8.1 구현 분석

**LlmJudge의 역할** (judge.rs):
- Structured output을 JSON schema로 강제
- `serde(default)` 어노테이션으로 누락 필드 허용 (partial response 대응)
- Recovery 메커니즘: 부분 JSON, 임베디드 JSON 추출

**Deep Review의 LLM 검사**:
- Semantic Quality: 4개 차원 (actionability, specificity, value-add, evidence quality) -- LLM이 0-100 점수 부여
- Cross-Artifact Consistency: 논리적 일관성, 모순 여부, 용어 일관성 -- LLM이 pass/fail 판정

### 8.2 효과성 한계

1. **LLM의 자기 생성물 평가 편향**: 생성과 평가 모두 LLM이 수행. 자신이 생성한 콘텐츠에 대해 관대할 수 있음. 코드에서 이를 완화하는 메커니즘은 programmatic override뿐.

2. **Score 재현성 부재**: 동일 입력에 대해 LLM이 다른 점수를 줄 수 있음. `check_with_thinking()`에서 `uncertainty` 파라미터로 이를 일부 반영하지만, 실제 측정은 외부에서 전달받는 값.

3. **프롬프트의 평가 기준 모호성**: `build_semantic_quality_prompt()`에서 "Use your judgment on what constitutes passing quality based on project context"라고 기술 (deep_review.rs line 655). 이는 평가 기준을 LLM에 위임하는 것.

---

## 9. Programmatic 검증이 잘못된 판단을 할 수 있는 구체적 시나리오

### 9.1 False Negative (환각을 놓치는 경우)

**시나리오 A: 참조 없는 환각**
- 콘텐츠: "이 프로젝트는 microservice 아키텍처를 사용하며 Kubernetes로 배포됩니다"
- 실제: monolith 프로젝트
- 결과: 파일 참조가 0이므로 `reference_validity = 1.0`, programmatic 검증 통과

**시나리오 B: 유효하지만 무관한 참조**
- 콘텐츠: "인증은 OAuth2를 사용합니다 [Verified: @src/config.rs:1]"
- 실제: src/config.rs line 1은 `use std::collections::HashMap;`
- 결과: 파일 존재 + 라인 범위 내이므로 programmatic 검증 통과

**시나리오 C: 임계값 이하 환각**
- 10개 참조 중 3개 무효 (30% < 기본 threshold 40%)
- 결과: LLM이 "not hallucinated"로 보고하면 통과

### 9.2 False Positive (유효한 콘텐츠를 환각으로 판정하는 경우)

**시나리오 D: 파일 경로 정규화 불일치**
- `VerifiedFileRegistry`의 경로 형식과 생성된 참조의 경로 형식이 다를 경우
- 코드 근거: `deep_review.rs` line 901-907에 일부 정규화 존재 (`trim_start_matches('@')`, `"./"`), 그러나 `file_reference.rs`의 `is_valid_file_ref()`에서는 직접 `registry.contains(path)` 호출
- 시나리오: `@./src/main.rs:42` vs `src/main.rs:42` 차이로 인한 오판 가능성

**시나리오 E: 빌드 시점과 검증 시점 파일 불일치**
- git 체크아웃이나 파일 생성 이후 `VerifiedFileRegistry`가 빌드되면 새 파일 참조가 invalid로 판정
- 코드 근거: `file_registry`는 `OnceCell`로 한 번만 빌드 (quality_loop.rs line 176-185)

---

## 10. 유형 분류/제한이 고가치 정보를 제약할 가능성

### 10.1 Issue Code 기반 분류 (issue_codes.rs)

14개의 `KnownIssueCode`가 정의됨. 각 이슈 코드는 적용 가능한 전략(`Semantic` 또는 `Evidence`)으로 매핑됨.

**제약 가능성**:
- `IssueCode::Unknown(String)` 처리 (line 39-42): Semantic + Evidence 모두 시도하므로 유연함
- 그러나 `identify_all_issues()` (engine.rs line 1039-1138)에서 이슈 코드에 따라 `DetectedIssue` 타입이 결정되며, 이는 전략 선택에 직접 영향

### 10.2 Artifact Category 기반 분류

**prune_hallucinated_artifacts()** (engine.rs line 1822):
```rust
if skill.artifact_category() == ArtifactCategory::ProjectSpecific && !result.validity.is_valid() {
    // 프루닝
}
```

`ProjectSpecific`이 아닌 아티팩트(예: 범용 규칙)는 환각이 있어도 프루닝되지 않음. 이는 범용 아티팩트를 보호하려는 의도적 설계.

### 10.3 리파인먼트 제한

**issues_per_iteration** 설정으로 한 iteration에서 처리하는 이슈 수 제한. Early termination (engine.rs line 1412-1422):
```rust
if quality_delta >= cfg.quality_acceptance_delta {
    // 유의미한 개선 달성시 남은 이슈 건너뛰기
    break;
}
```

이는 효율성을 위한 설계이지만, 남은 이슈들이 처리되지 않을 수 있음.

---

## 11. LLM 자율 판단 vs 프로그래매틱 강제의 균형점

### 11.1 현재 균형 (코드 분석 기반)

| 검증 차원 | 담당 | 비고 |
|-----------|------|------|
| 파일 참조 존재 | Programmatic (100%) | `is_valid_file_ref()` |
| 라인 범위 유효 | Programmatic (100%) | registry.line_count 대조 |
| 환각 판정 | Hybrid (LLM 우선 + Programmatic override) | threshold 기반 |
| 의미적 품질 | LLM 전담 | LlmJudge, Deep Review |
| 구조적 유효성 | Programmatic (100%) | ArtifactValidator |
| 중복 탐지 | Programmatic (이름), LLM (의미) | ConsistencyResult |
| 교차 일관성 | LLM 전담 | Deep Review |
| 수렴 판정 | Hybrid | QualityAssessor + metrics |
| 전략 선택 | Hybrid | IssueCode 매핑 + StrategyRotator 이력 |

### 11.2 설계 원칙 (코드 주석/doc에서 추출)

1. **"Safety-only validation"** (validators.rs line 3-4): Programmatic 검증은 구조적 무결성과 환각 탐지에 집중
2. **"Content quality is evaluated by LlmJudge"** (validators.rs line 4): 의미적 품질은 LLM에 위임
3. **"Programmatic check overrides LLM when >30% of file references are invalid"** (judge.rs line 325-327): 팩트 기반 override 허용
4. **"LLM reporting hallucinated=true is always trusted"** (judge.rs line 327): LLM의 자기보고 환각은 무조건 신뢰

---

## 12. 검증이 놓치는 품질 차원

### 12.1 정확성 (Accuracy)
- Programmatic: 파일 참조 정확성만 검증
- LLM: 의미적 정확성 평가 시도하지만, 프로젝트 도메인 지식 부족으로 한계
- **누락**: 주장의 기술적 정확성 (예: "이 함수는 O(n log n) 복잡도" 같은 주장의 사실 여부)

### 12.2 유용성 (Usefulness)
- LLM Judge의 `actionability` 점수로 부분 커버
- **누락**: 실제 개발자의 작업 흐름에서의 유용성 (개발자 피드백 없이 평가)

### 12.3 완전성 (Completeness)
- `PlanConsistencyResult`: overview 존재, 최소 아티팩트 존재 확인
- Structural validation: 모듈 커버리지 확인
- **누락**: 프로젝트의 핵심 개념/패턴이 모두 문서화되었는지 (도메인 지식 필요)

### 12.4 최신성 (Currency)
- `VerifiedFileRegistry`가 실행 시점의 파일 상태 반영
- **누락**: 참조된 코드 패턴이 최신 관행인지 (deprecation 등)

### 12.5 가독성 (Readability)
- **완전 누락**: 생성된 문서의 가독성, 구조 명확성은 검증하지 않음
- LLM Deep Review가 부분적으로 커버하지만 명시적 가독성 메트릭 없음

### 12.6 중복 탐지 (Cross-artifact Deduplication)
- ConsistencyResult: 이름 중복만 검출
- **누락**: 의미적 중복 (다른 이름의 같은 내용, 여러 아티팩트에 걸친 중복 지침)

---

## 13. 종합 평가

### 13.1 강점 요약

1. **다중 계층 방어**: 5개 층위의 검증 + outer quality loop의 다중 게이트로 구성된 방어 전략은 단일 실패 지점을 효과적으로 방지한다.

2. **Programmatic + LLM Hybrid**: 결정론적으로 검증 가능한 것(파일 존재, 라인 범위, 중복)은 programmatic으로, 의미적 판단이 필요한 것은 LLM으로 분리한 설계가 명확하다.

3. **수렴 보장**: max_iterations, 진동 탐지, 정체 탐지, best state 보존 등 다중 안전장치로 무한 루프를 방지한다.

4. **Selective Assessment 최적화**: 수정된 아티팩트만 재평가하여 LLM 호출 비용을 ~85% 절감한다.

5. **Graceful Degradation**: Recovery 메커니즘(partial JSON, embedded JSON), best state 반환 등으로 실패 시에도 최선의 결과를 제공한다.

### 13.2 핵심 리스크

1. **참조 없는 콘텐츠에 대한 환각 탐지 공백**: `reference_validity = 1.0`으로 자동 통과하는 설계가 가장 큰 맹점. LLM Judge에만 의존하게 됨.

2. **의미적 참조 정확성 미검증**: 파일 존재와 라인 범위만 확인하고, 참조된 코드의 내용이 주장과 일치하는지는 검증하지 않음.

3. **LLM 자기 평가 편향**: 생성과 평가를 동일 LLM이 수행하며, 이를 완화하는 외부 검증 메커니즘이 제한적.

4. **임계값의 경험적 성격**: 0.4 (환각 threshold), 0.9 (reference validity gate), 0.85 (target quality) 등의 매직 넘버가 경험적으로 설정됨. 프로젝트 특성에 따라 최적값이 다를 수 있음.

### 13.3 개선 가능 방향 (사실 기반 관찰)

1. **참조 콘텐츠 검증**: `is_valid_file_ref()`를 확장하여 참조 라인의 실제 코드 스니펫과 주장의 관련성을 검증하는 것은 기술적으로 가능하나, 현재 미구현.

2. **다양한 모델 교차 검증**: 생성 모델과 다른 모델로 평가하는 것은 `ProviderSet`의 phase-based routing으로 가능한 인프라가 이미 존재 (quality_loop.rs에서 `providers.provider_for_phase(phase_id::DEEP_REVIEW)` 사용).

3. **참조 없는 주장 탐지**: 주장 문장에서 파일 참조가 없는 기술적 주장을 식별하고 경고하는 것은 패턴 매칭으로 구현 가능하나, false positive 비율이 높을 수 있음.
