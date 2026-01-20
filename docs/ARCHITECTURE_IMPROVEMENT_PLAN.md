# Claudegen 아키텍처 개선 계획서

> 작성일: 2025-01-20
> 상태: 승인 대기
> 우선순위: Critical

---

## 1. 현황 요약

### 1.1 발견된 문제

프로젝트 전체 심층 분석 결과, **매 리뷰마다 심각한 이슈가 반복 발견되는 근본 원인** 7가지가 식별됨:

| # | 근본 원인 | 위치 | 심각도 |
|---|----------|------|--------|
| 1 | 검증 루프 조기 종료 (90%에서 OK) | `refinement.rs:764-838` | Critical |
| 2 | Bottom-Up 분석 부재 | `pipeline/analysis/` (삭제됨) | High |
| 3 | 검증기 간 Pass/Fail 로직 불일치 | `validation/` 전체 | Critical |
| 4 | 하드코딩된 임계값 60개 이상 | 전체 코드베이스 | High |
| 5 | 정적 키워드 기반 분류 (690개 패턴) | `patterns.rs`, `knowledge_classifier.rs` | Critical |
| 6 | DeepAnalysis 결과 미활용 | `insight/mod.rs` | High |
| 7 | 무한 루프 방지가 wall-clock에만 의존 | `feedback_loop.rs:373-590` | High |

### 1.2 전체 이슈 통계

| 심각도 | 개수 |
|--------|------|
| Critical | 7 |
| High | 14 |
| Medium | 18 |
| Low | 10 |
| **총계** | **49** |

---

## 2. 키워드/패턴 매칭 시스템 분석

### 2.1 현재 정적 패턴 규모

| 파일 | 패턴 수 | 용도 |
|------|--------|------|
| `patterns.rs` | ~400개 | Tier 1 필터링 |
| `knowledge_classifier.rs` | ~80개 | Tier 분류, 아티팩트 라우팅 |
| `constraint_detector.rs` | ~50개 | 제약사항 탐지 |
| `value_scorer.rs` | ~40개 | 가치 평가 |
| `project_consistency.rs` | ~40개 | 프로젝트 타입 검증 |
| `tier_filter.rs` | ~30개 | Tier 1 이름 필터 |
| `output_router.rs` | ~20개 | 아티팩트 라우팅 |
| `agents.rs`, `skill.rs` | ~30개 | 타입별 Tier 판별 |
| **총계** | **~690개** | |

### 2.2 키워드 매칭의 문제점

```rust
// 현재 코드 (patterns.rs:101-530)
pub const TIER1_PATTERNS: &[&str] = &[
    "cargo build", "npm install", "handle errors",
    "use async/await", "write tests"...
];

// 문제 사례
"handle errors using ClaudegenError" → 잘못된 거부 (프로젝트 특화인데 Tier 1)
"use async/await for all LLM calls" → 잘못된 거부 (컨텍스트 무시)
```

### 2.3 키워드 vs LLM 비교

| 기준 | 키워드 매칭 | LLM |
|------|------------|-----|
| 정확도 | ~60% | ~95% |
| 속도 | <1ms | ~500ms (배치) |
| 비용 | $0 | ~$0.50/1000개 |
| 유지보수 | 수동 업데이트 | 자동 적응 |
| 프로젝트 특화 | 불가능 | 가능 |

### 2.4 결정: 하이브리드 접근

```
┌─────────────────────────────────────────────┐
│           하이브리드 분류 파이프라인          │
├─────────────────────────────────────────────┤
│                                             │
│  [Level 1: 구조적 체크] ← 확정적 규칙만     │
│  - 파일 참조 유무                           │
│  - 최소 길이                                │
│  - 필수 섹션 존재                           │
│       ↓                                     │
│  통과 / 실패(명확) / 불확실                 │
│       ↓                                     │
│  [Level 2: LLM 판단] ← 불확실한 케이스      │
│  - Tier 분류                                │
│  - 아티팩트 타입                            │
│  - 가치 평가                                │
│  - 프로젝트 특화도                          │
│                                             │
└─────────────────────────────────────────────┘
```

---

## 3. 코드 변경 계획

### 3.1 삭제할 코드

```
src/pipeline/patterns.rs
  삭제:
  - TIER1_PATTERNS (~400개)
  - TIER3_INDICATORS (~20개)
  - TIER1_SKILL_NAMES (~40개)
  - TIER1_AGENT_NAMES (~40개)
  - VALUE_INDICATORS (~15개)

  유지:
  - FILE_REF, FILE_LINE_REF (구조적 패턴)
  - CODE_EXAMPLE_PATTERN (마크다운 문법)

src/pipeline/insight/knowledge_classifier.rs
  삭제:
  - BUILTIN_TIER0/2/3
  - RULE/SKILL/AGENT_KEYWORDS

src/pipeline/insight/constraint_detector.rs
  삭제:
  - CONCURRENCY_KEYWORDS
  - SECURITY_KEYWORDS
  - GOTCHA_KEYWORDS

src/pipeline/validation/project_consistency.rs
  삭제:
  - CLI/BACKEND/FRONTEND/LIBRARY_KEYWORDS
```

### 3.2 신규 생성 파일

```
src/pipeline/insight/llm_classifier.rs (신규)
  - TierClassifier trait
  - LlmTierClassifier struct
  - 배치 분류 지원

src/pipeline/insight/hybrid_classifier.rs (신규)
  - HybridClassifier struct
  - 구조적 체크 + LLM 조합
  - 캐싱 레이어

src/pipeline/validation/llm_validator.rs (신규)
  - LLM 기반 검증
  - 프로젝트 타입 검증
```

### 3.3 수정할 파일

```
src/config/types.rs
  추가:
  - LlmClassificationConfig
  - 분류별 모델 선택
  - 캐시 설정

src/pipeline/insight/mod.rs
  - HybridClassifier 통합
  - DeepAnalysis 직접 활용

src/pipeline/refinement.rs
  - 조기 종료 조건 수정 (90% → 설정값)

src/pipeline/quality_loop.rs
  - Deep review 우회 방지
```

---

## 4. 설정 추가 계획

```toml
# claudegen.toml에 추가될 설정

[classification]
# 분류에 사용할 모델
model = "claude-haiku-4-5"
# 배치 크기
batch_size = 10
# 신뢰도 임계값 (미달 시 더 강력한 모델 사용)
confidence_threshold = 0.8
fallback_model = "claude-sonnet-4-5"

[classification.cache]
enabled = true
ttl_hours = 24

[convergence]
# 현재 하드코딩된 0.9 → 설정으로
no_issues_quality_floor = 1.0
oscillation_lenient_passes = 2

[generation.thresholds]
# 현재 하드코딩된 값들
min_skill_value = 0.5
min_agent_value = 0.6
min_rule_value = 0.4

[validation.evidence]
min_file_refs_per_100_chars = 1
validate_line_numbers = true

[refinement.strategies.semantic]
model = "claude-opus-4-5"

[refinement.strategies.evidence]
model = "claude-sonnet-4-5"
```

---

## 5. 구현 단계

### Phase 1: 기반 작업 (예상: 1주)

1. **LlmClassifier 인터페이스 정의**
   ```rust
   #[async_trait]
   pub trait LlmClassifier: Send + Sync {
       async fn classify_tier(&self, insight: &Insight) -> Result<TierClassification>;
       async fn classify_artifact(&self, insight: &Insight) -> Result<ArtifactType>;
       async fn batch_classify(&self, insights: &[Insight]) -> Result<Vec<Classification>>;
   }
   ```

2. **캐싱 레이어 구현**
   - 콘텐츠 해시 기반 캐시 키
   - TTL 기반 만료
   - LRU 정책

3. **배치 처리 인프라**
   - 여러 인사이트를 한 번의 LLM 호출로 처리
   - JSON 배열 응답 파싱

### Phase 2: 분류 시스템 교체 (예상: 2주)

1. **knowledge_classifier.rs 전환**
   - 키워드 기반 → 하이브리드 방식
   - 구조적 체크 우선, LLM 후속

2. **tier_filter.rs 개선**
   - LLM 기반 검증 추가
   - 파일 참조 검증 강화

3. **constraint_detector.rs 전환**
   - 키워드 기반 → LLM 기반 탐지
   - 코드 의미 이해 기반 제약 탐지

### Phase 3: 검증 시스템 통합 (예상: 1주)

1. **검증기 통일**
   - 공통 Severity enum
   - 일관된 Pass/Fail 로직

2. **patterns.rs 정리**
   - 불필요한 패턴 제거
   - 구조적 패턴만 유지

3. **회귀 테스트**
   - 기존 테스트 케이스 유지
   - 새로운 분류 정확도 테스트

### Phase 4: 설정 및 마무리 (예상: 1주)

1. **설정 노출**
   - Config 타입 추가
   - 기본값 설정

2. **문서화**
   - CLAUDE.md 업데이트
   - 설정 가이드

3. **성능 벤치마크**
   - 분류 정확도 측정
   - 속도/비용 측정

---

## 6. 검증 루프 개선 설계

### 현재 문제
```
[검증] → 90%면 OK → 종료 (품질 미달 가능)
```

### 개선된 설계
```
┌─────────────────────────────────────────────────────────────┐
│                    다중 레벨 검증 루프                        │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  Level 1: 구조적 검증 (빠름)                                │
│  - 최소 파일 참조 개수                                      │
│  - Tier 1 패턴 검출                                         │
│  - 필수 섹션 존재                                           │
│       ↓ 통과시                                              │
│                                                             │
│  Level 2: 의미적 검증 (LLM)                                 │
│  - 액션 가능성                                              │
│  - 구체성                                                   │
│  - 중복성 (전체 아티팩트 교차)                              │
│       ↓ 통과시                                              │
│                                                             │
│  Level 3: 증거 검증                                         │
│  - 파일 존재 확인                                           │
│  - 라인 번호 유효성                                         │
│  - 코드 스니펫 일치                                         │
│       ↓ 통과시                                              │
│                                                             │
│  Level 4: 교차 아티팩트 검증                                │
│  - 역할 명확성                                              │
│  - 모듈 커버리지 균형                                       │
│  - 일관성                                                   │
│       ↓ 통과시                                              │
│                                                             │
│  Level 5: Deep Review                                       │
│  - LLM 기반 전체 품질 평가                                  │
│  - Claude Code 최적화 검증                                  │
│                                                             │
│  [수렴 조건] (모두 충족시 종료)                             │
│  1. quality_score >= target_quality (100%, not 90%)        │
│  2. consecutive_passes >= required_passes                  │
│  3. oscillation_detected == false                          │
│  4. all_validators_passed == true                          │
│                                                             │
│  [종료 보장]                                                │
│  - max_iterations (하드 리밋)                               │
│  - max_llm_calls (예산 기반)                                │
│  - wall_clock_timeout (시간 기반)                           │
│  - stagnation_limit (개선 없음 횟수)                        │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

---

## 7. 예상 효과

| 메트릭 | 현재 | 개선 후 |
|--------|------|--------|
| Tier 분류 정확도 | ~60% | ~95% |
| 잘못된 거부율 | ~25% | <5% |
| 잘못된 수락률 | ~15% | <5% |
| 유지보수 필요 패턴 | 690개 | ~20개 |
| 프로젝트 특화도 | 낮음 | 높음 |

---

## 8. 위험 요소 및 완화

| 위험 | 영향 | 완화 방안 |
|------|------|----------|
| API 비용 증가 | ~$0.50/1000개 | Haiku 사용, 캐싱, 배치 처리 |
| 속도 저하 | ~500ms/배치 | 배치 처리, 병렬화 |
| LLM 불확실성 | 가끔 다른 결과 | 캐싱, confidence threshold |

---

## 9. 다음 단계

1. 이 계획 승인
2. Phase 1 구현 시작: `LlmClassifier` trait 및 캐싱 레이어
3. 점진적 마이그레이션 (기존 코드 병행 운영)

---

## 부록 A: 관련 파일 목록

### 수정 대상 (Critical)
- `src/pipeline/refinement.rs`
- `src/pipeline/quality_loop.rs`
- `src/pipeline/patterns.rs`
- `src/pipeline/insight/knowledge_classifier.rs`
- `src/pipeline/insight/value_scorer.rs`
- `src/pipeline/insight/constraint_detector.rs`
- `src/pipeline/validation/tier_filter.rs`

### 수정 대상 (High)
- `src/config/types.rs`
- `src/pipeline/insight/mod.rs`
- `src/pipeline/validation/project_consistency.rs`
- `src/pipeline/generation/artifact/*.rs`

### 신규 생성
- `src/pipeline/insight/llm_classifier.rs`
- `src/pipeline/insight/hybrid_classifier.rs`
- `src/pipeline/validation/llm_validator.rs`

---

## 부록 B: 하드코딩 임계값 목록

| 위치 | 값 | 설명 | 설정 키 제안 |
|------|---|------|-------------|
| `refinement.rs:764` | 0.9 | 품질 바닥 | `convergence.no_issues_quality_floor` |
| `quality_loop.rs:312` | 0.3 | 증거 무효 비율 | `validation.evidence.invalid_ratio_max` |
| `quality_loop.rs:330` | 5 | 에스컬레이션 갭 수 | `quality.escalation_gap_threshold` |
| `generation/skills.rs:15` | 0.5 | 최소 스킬 가치 | `generation.thresholds.min_skill_value` |
| `generation/agents.rs:15` | 0.6 | 최소 에이전트 가치 | `generation.thresholds.min_agent_value` |
| `generation/rules.rs:16` | 0.4 | 최소 규칙 가치 | `generation.thresholds.min_rule_value` |
| `convergence.rs:109` | 0.9 | 엄격 모드 임계값 | `convergence.strict_mode_threshold` |
| `feedback_loop.rs:418` | 0.9 | 참조 검증 비율 | `validation.reference.min_ratio` |
| `deep_review.rs:129` | 0.7 | 체크 통과 임계값 | `deep_review.check_pass_threshold` |
| `tier_filter.rs:430` | 2 | 최소 파일 참조 | `validation.min_file_refs` |
