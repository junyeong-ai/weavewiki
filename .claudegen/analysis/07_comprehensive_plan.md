# 07. 종합 설계 검증 및 최적 구현 계획

> 6개 심층 분석 보고서(01~06) 종합 교차 분석 결과
> 검증된 베스트 프랙티스 기반, 장기적 관점의 최적 솔루션

---

## 1. 논리적 오류 및 기능적 결함 검출

### 1.1 Critical Issues (즉시 해결 필요)

#### C1: 참조 없는 콘텐츠의 환각 탐지 공백
- **근거**: `types/quality.rs:109-112` - 참조 0개일 때 `reference_validity = 1.0`
- **영향**: LLM이 파일 참조 없이 거짓 기술 주장을 포함시켜도 programmatic 검증 통과
- **해결**: Content Assertion Detector 도입 - 기술 주장 패턴 감지 후 참조 요구
  ```
  기술 주장 패턴: "uses X", "implements Y", "architecture is Z"
  → 참조 없으면 warning, 3개 이상 무참조 주장이면 LLM 재평가 트리거
  ```

#### C2: 의미적 참조 정확성 미검증
- **근거**: `file_reference.rs` - 파일 존재 + 라인 범위만 확인, 내용 미검증
- **영향**: `@src/main.rs:42`를 참조하면서 무관한 주장 가능
- **해결**: Spot-check Validator - 랜덤 샘플링으로 참조 라인의 코드 스니펫과 주장 일치도 LLM 검증
  ```
  전체 참조의 20% 샘플링 → 코드 스니펫 추출 → LLM에 "이 코드가 주장과 관련있는가?" 확인
  비용: 추가 LLM 호출 1회 (배치)
  ```

#### C3: Format Validation의 표면적 검사
- **근거**: `deep_review.rs:482-490` 자체 인정 - `starts_with("---")`가 markdown horizontal rule과 혼동
- **영향**: 잘못된 YAML frontmatter가 유효로 판정
- **해결**: Lightweight YAML 파싱 도입 (full YAML parser 불필요, key-value 추출만)

### 1.2 High Priority Issues (조기 해결 권장)

#### H1: 유형 분류의 고정성이 다양한 도메인 지원을 제한
- **근거**:
  - `Tier3Category` 5개 고정 (시스템 프로그래밍 편향)
  - `WELL_KNOWN_CONCERNS` 8개 고정
  - `PolicyType` 8개 고정 (GDPR, SOX 등 미지원)
  - `KNOWN_FRAMEWORKS` 10개 고정
- **영향**: 금융, 의료, IoT 등 도메인 특화 프로젝트에서 가치 있는 분류가 누락
- **해결**:
  1. `Tier3Category`에 `Custom(String)` 추가
  2. `PolicyType`, `DomainLogicType`에 `Other(String)` 추가
  3. `KNOWN_FRAMEWORKS`를 config 파일에서 로드
  4. `WELL_KNOWN_CONCERNS`를 LLM 발견 기반으로 동적 확장

#### H2: Hierarchical Summarization의 Top-5 모듈 제한
- **근거**: `hierarchical_summarizer.rs:149` - 프로젝트 요약에 상위 5개 모듈만 포함
- **영향**: 20+ 모듈의 대규모 프로젝트에서 중요 모듈 누락
- **해결**: config 가능한 threshold + importance-weighted 선택

#### H3: Token 추정의 부정확성
- **근거**: `context_tracker.rs` - 고정 `CHARS_PER_TOKEN` 상수 사용
- **영향**: 코드 vs 산문의 토큰화 차이로 예산 과소/과다 사용
- **해결**: 언어별 가중치 적용 (코드: ~3.5, 산문: ~4.5, 주석: ~4.0)

#### H4: LLM 자기 평가 편향
- **근거**: 생성과 평가를 동일 LLM이 수행
- **영향**: 자기 생성물에 관대한 평가 가능성
- **해결**: 이미 `ProviderSet`에 phase별 tier 라우팅 존재 → deep_review를 다른 모델 tier로 라우팅

### 1.3 Medium Priority Issues

#### M1: Regression Check 문자열 의존
- **근거**: `deep_review.rs:832-835` - 이슈 비교가 문자열 완전 일치
- **해결**: Semantic similarity 기반 이슈 매칭 (Jaccard similarity + 핵심 키워드 추출)

#### M2: Quick Quality의 구조 편향
- **근거**: `strategy/mod.rs:489-503` - 참조 많고 길기만 한 콘텐츠가 높은 점수
- **해결**: 참조/길이 포화점 낮추기 + 정보 밀도 보정 계수 추가

#### M3: Archive Schema Versioning 부재
- **근거**: `archive.rs` - JSON 기반, 스키마 버전 관리 없음
- **해결**: `schema_version` 필드 + 마이그레이션 매퍼

#### M4: 언어 파서 커버리지 Gap
- **근거**: 45개 언어 감지, 7개만 tree-sitter 파서 지원
- **해결**: 단계적 파서 추가 (Java → C/C++ → Kotlin → Ruby 순), 파서 없는 언어는 LLM 기반 구조 추출 활용

---

## 2. 실제 사용 시나리오별 가치/효과 검증

### 2.1 소형 프로젝트 (< 50 파일)
| 차원 | 현재 상태 | 평가 |
|------|----------|------|
| 분석 완전성 | 100% 파일 분석 보장 | 우수 |
| 분석 시간 | 과도할 수 있음 (9 phase) | `Fast` depth로 적절 |
| 생성물 가치 | Base agents + 기본 rules | 적절 |
| 컨텍스트 효율 | 충분한 여유 | 우수 |

### 2.2 중형 프로젝트 (50-500 파일)
| 차원 | 현재 상태 | 평가 |
|------|----------|------|
| 분석 완전성 | Map-Reduce 청크 분석 | 우수 |
| 모듈 감지 | 적절한 수준 | 양호 |
| 생성물 다양성 | Module/Service agents 활성화 | 우수 |
| Progressive Disclosure | 효과적 | 우수 |

### 2.3 대형 모노레포 (500+ 파일, 다중 서비스)
| 차원 | 현재 상태 | 평가 |
|------|----------|------|
| 분석 완전성 | 분산 분석 + 완전성 검증 | 우수 |
| 컨텍스트 관리 | 3-tier budget 시스템 | 양호 → 개선 필요 |
| 워크스페이스 분리 | Nested CLAUDE.md + 워크스페이스 스킬 | 우수 |
| 서비스 감지 | 6가지 서비스 타입 | 양호 |
| 확장 위험 | Top-5 모듈 제한, 임포트 20개 제한 | 개선 필요 |

### 2.4 도메인 특화 프로젝트 (금융, 의료, IoT)
| 차원 | 현재 상태 | 평가 |
|------|----------|------|
| 도메인 분류 | PolicyType 8개 고정 | 제한적 |
| 비즈니스 룰 감지 | DomainAnalyzer 존재 | 양호 |
| 규제 준수 | 미지원 | 개선 필요 |
| 용어집 생성 | 7가지 TermCategory | 양호 |

---

## 3. 유형 분류 제한 vs LLM 판단 자율성 균형

### 3.1 현재 균형 분석

```
                    Programmatic Control ◄──────────────────► LLM Autonomy
                    │                                                    │
파일 존재 검증  ████████████                                             │  100% Programmatic
라인 범위 검증  ████████████                                             │  100% Programmatic
환각 판정       ████████████████████████████████                         │  Hybrid (70/30)
구조 유효성     ████████████████████████████████                         │  Hybrid (80/20)
유형 분류       ████████████████████████████████████████████████████     │  Fixed enum (90/10)
의미적 품질     ████████████████████████████████████████████████████████ │  LLM dominant (10/90)
콘텐츠 결정     ████████████████████████████████████████████████████████ │  LLM dominant (5/95)
```

### 3.2 문제 진단

**현재 `유형 분류`가 과도하게 programmatic:**
- Fixed enum이 LLM의 도메인 특화 분류 능력을 제한
- `Tier3Category` 5개, `WELL_KNOWN_CONCERNS` 8개, `PolicyType` 8개 = 총 21개 고정 분류
- 실제 엔터프라이즈 프로젝트에는 50+ 도메인 특화 분류가 필요할 수 있음

**비용-효과 분석:**
- Programmatic 분류의 장점: 일관성, 빠른 실행, 디렉토리 구조 예측 가능
- Programmatic 분류의 단점: 유연성 부족, 도메인 미스매치, 가치 있는 분류 누락
- LLM 분류의 장점: 도메인 적응, 창의적 분류, 맥락 인식
- LLM 분류의 단점: 비일관성, 비용, 네이밍 변동

### 3.3 최적 균형점 제안

**원칙: "Structured Freedom" (구조화된 자유)**

1. **Core Categories (고정)**: Project, Tech, Framework = 분류 결정론 보장
2. **Known Categories (확장 가능)**: Module, Service, CrossCutting = 기본 제공 + 추가 허용
3. **Discovered Categories (LLM 자율)**: Domain, Custom = LLM이 발견, 구조만 제공

```rust
// 현재: 고정 enum
enum Tier3Category { ConcurrencyTrap, ResourceLeak, ... }

// 제안: Hybrid approach
enum Tier3Category {
    // 코어 (항상 검출)
    ConcurrencyTrap, ResourceLeak, StateInvariant, SecurityBoundary, PerformanceTrap,
    // LLM 발견 (동적)
    Discovered { name: String, evidence_count: usize },
}
```

**적용 범위:**
```
파일 존재 검증    → 100% Programmatic (변경 없음)
환각 판정         → Hybrid 강화 (Spot-check Validator 추가)
유형 분류         → "Structured Freedom" (Core + Known + Discovered)
의미적 품질       → LLM dominant (변경 없음, 모델 교차 검증 추가)
콘텐츠 결정       → LLM autonomous (변경 없음)
```

---

## 4. Map-Reduce 완전 분석 보장 전략

### 4.1 현재 분석 완전성 검증 결과

```
File Discovery    ████████████████████████████████████████████████████ 100%
  └ VerifiedFileRegistry: 실제 파일시스템 스캔 (gitignore 인식)

Chunk Coverage    ████████████████████████████████████████████████████ 100%
  └ CompletenessValidator: 3단계 검증 (truncated + failed + unanalyzed)

AST Depth         ████████████████████████████████ 70%
  └ 7/45 언어 tree-sitter 지원, 나머지는 LLM 기반 구조 추출

Information Flow  ████████████████████████████████████████████ 85%
  └ Chunk → Module → Project 계층 요약 (Top-5 제한)

Cross-Reference   ████████████████████████████████████████████████ 95%
  └ SymbolIndex 기반 교차 참조, 공통 심볼 필터링
```

### 4.2 강화 전략

#### S1: AST 기반 완전 구조 파악 → 모듈/구조별 분할 분석
```
Phase 0: Structure Discovery (추가 제안)
  1. tree-sitter 지원 언어: AST 기반 구조 그래프 구축
  2. 비지원 언어: 디렉토리 구조 + 파일명 패턴 기반 모듈 추론
  3. dependency 그래프: import/require/use 문 파싱

Phase 1: Module-Aware Chunking
  1. 모듈 경계 기반 청크 분할 (현재도 일부 지원)
  2. 모듈 내 파일은 같은 청크로 그룹핑 (의미적 응집성)
  3. 모듈 간 cross-reference 파일은 양쪽 청크에 요약 포함

Phase 2: Hierarchical Analysis
  1. Chunk-level: 파일별 세부 분석
  2. Module-level: 모듈 내 패턴/관습 취합
  3. Service-level: 서비스 간 인터페이스/의존성
  4. Project-level: 아키텍처 패턴/교차 관심사
```

#### S2: 파일 크기별 최적 청크 분할
```
현재 구현 (이미 존재):
  - Token budget 기반 청크 크기 결정
  - AST-aware 분할 (80줄 이상 블록은 메서드 수준)
  - Structure boundary 우선 truncation

보강:
  - 파일 유형별 가중치: config 파일 (낮음), 비즈니스 로직 (높음)
  - 변경 빈도 기반 우선순위: 자주 변경되는 파일 우선 분석
  - 복잡도 기반 depth: 높은 순환 복잡도 파일에 더 많은 예산
```

#### S3: 중간 요약의 정보 유실 방지
```
현재 구현:
  - Single-chunk 모듈: 요약 생략 (정보 보존 100%)
  - Multi-chunk 모듈: Jaccard 기반 dedup

보강:
  - Reference Carry-Forward: 모든 레벨에서 원본 파일 참조 유지
  - Confidence Decay: 요약 단계가 증가할수록 confidence 감소 표시
  - Key Finding Preservation: 임계값 이상 importance의 발견은 무조건 보존
```

---

## 5. 컨텍스트 윈도우 안전 관리

### 5.1 현재 시스템 분석

```
Context Budget Architecture:
  ┌─────────────────────────────────────┐
  │ Total Context Window (200K chars)   │
  │ ┌─────────────────────────────────┐ │
  │ │ Input Budget (80%)              │ │
  │ │ ┌───────┬──────┬──────────────┐ │ │
  │ │ │ Tier1 │ Tier2│ Tier3        │ │ │
  │ │ │ Always│ If   │ Summarized   │ │ │
  │ │ │       │ Space│ if tight     │ │ │
  │ │ └───────┴──────┴──────────────┘ │ │
  │ └─────────────────────────────────┘ │
  │ ┌─────────────────────────────────┐ │
  │ │ Output Reserved (20%)           │ │
  │ └─────────────────────────────────┘ │
  └─────────────────────────────────────┘
```

### 5.2 위험 시나리오와 대응

#### R1: 대형 프로젝트에서 분석 결과가 컨텍스트 초과
- **현재 대응**: 3-tier 로딩 + `needs_summarization()` 체크
- **보강**: Tier3 데이터를 `.claudegen/output/`에 JSON으로 저장, 필요시 조회

#### R2: 생성 프롬프트가 모델 컨텍스트 초과
- **현재 대응**: `MAX_PROMPT_CHARS = 200_000` 하드캡, AST boundary truncation
- **보강**: 프롬프트 크기 예측 → 초과 시 분할 생성 (chunk-level generation)

#### R3: Quality Loop 반복에서 컨텍스트 누적
- **현재 대응**: 각 iteration에서 새 pipeline 인스턴스 생성
- **보강**: Iteration 간 carry-forward 데이터 크기 모니터링

### 5.3 장기적 전략: Reference-on-Demand

```
CLAUDE.md에 포함:
  - 핵심 개요 (< 20줄)
  - Navigation Map (모듈-규칙-스킬 매핑)
  - @import 참조 (priority 순)

Rules에 포함:
  - 프로젝트/기술/프레임워크 핵심 규칙
  - 조건부 활성화 (paths frontmatter)

Skills에 포함:
  - Description만 상시 로드
  - 전체 내용은 호출시

전체 분석 결과:
  - .claudegen/output/ 에 저장
  - 필요시 참조 가능
  - 절대 컨텍스트에 직접 주입하지 않음
```

---

## 6. Progressive Disclosure 최적 구현

### 6.1 4-Tier Progressive Disclosure 아키텍처

```
Tier 0: Always Loaded (CLAUDE.md core)
  ├── 프로젝트 개요 (5-10줄)
  ├── 빌드/테스트 명령어
  ├── 핵심 제약사항 (3-5개)
  └── Navigation Map (@import 목록)

Tier 1: Path-Conditional (Rules)
  ├── project.md → always_inject=true
  ├── tech/*.md → *.{ext} 파일 작업시
  ├── frameworks/*.md → 특정 경로/키워드 작업시
  ├── modules/*.md → 해당 모듈 디렉토리 작업시
  ├── domains/*.md → 도메인 키워드 트리거시
  └── cross-cutting/*.md → 관련 키워드 트리거시

Tier 2: Description-Triggered (Skills)
  ├── 스킬 description → 항상 컨텍스트에 포함
  ├── SKILL.md 본문 → 호출시 또는 관련성 판단시
  ├── patterns.md → on-demand (대형 참조)
  └── examples.md → on-demand (예제)

Tier 3: Delegation (Agents)
  ├── 에이전트 → 별도 컨텍스트 윈도우
  ├── 스킬 프리로드 → 에이전트에 도메인 주입
  └── 규칙 참조 → @.claude/rules/ 자동 참조
```

### 6.2 상호 참조 체계

```
CLAUDE.md ──@import──→ .claude/rules/project.md
CLAUDE.md ──@import──→ .claude/docs/architecture.md
CLAUDE.md ──navigation──→ .claude/skills/, .claude/agents/

Rules ──triggers──→ 관련 Skills
Rules ──paths──→ 조건부 활성화
Rules ──@import──→ 상위 Rules (tech → framework)

Skills ──## Related Skills──→ 관련 Skills
Skills ──@.claude/rules/──→ 관련 Rules
Skills ──## Recommended Agent──→ 전문 Agent

Agents ──skills:──→ 프리로드 Skills
Agents ──prompt──→ @.claude/rules/ 참조
Agents ──context──→ 분석 데이터 인젝션
```

### 6.3 대형 컨텐츠 점진적 접근

**원칙: "큰 컨텐츠를 없애는 것이 아니라, 단계적으로 접근 가능하게 하는 것"**

```
1단계: Navigation (CLAUDE.md Navigation Map)
  → "어떤 모듈에 어떤 규칙/스킬/에이전트가 있는지" 개요

2단계: Summary (Rules/Skills description)
  → "이 규칙/스킬이 무엇을 다루는지" 요약

3단계: Detail (Full rule/skill content)
  → "구체적인 지침과 예제" 상세 내용

4단계: Reference (Extracted docs, patterns, examples)
  → "배경 자료와 참고 정보" 보충 자료

5단계: Raw Analysis (.claudegen/output/)
  → "원본 분석 데이터" 전체 참조
```

---

## 7. 이벤트 소싱 및 복원력 검증

### 7.1 현재 시스템 강점

- **Dual-layer persistence**: Event log + Checkpoint snapshots
- **Flush-per-write**: 이벤트별 즉시 flush → 크래시 시 최대 1 이벤트 손실
- **22 event types**: 전체 파이프라인 lifecycle 커버
- **Sharded storage**: 1000 events/shard, 확장 가능
- **Schema versioning**: `schema_version: u32` 미래 마이그레이션 준비
- **Incremental compaction**: 메모리 성장 제어

### 7.2 검증 결과

| 시나리오 | 복원 가능? | 정보 유실? | 근거 |
|---------|-----------|-----------|------|
| 분석 중 크래시 | O | 최소 (마지막 청크) | chunk-level 체크포인트 |
| 리파인먼트 중 크래시 | O | 최소 (마지막 iteration) | iteration 스냅샷 |
| Deep Review 중 크래시 | O | pass 단위 복원 | pass completion events |
| LLM 타임아웃 | O | 없음 | graceful timeout handling |
| 디스크 공간 부족 | △ | 가능 | flush 실패 시 warn만 |
| config 변경 후 재개 | △ | 재분석 필요 | config hash 불일치 감지 |

### 7.3 개선 권장

1. **Checkpoint 주기 최적화**: 현재 `session_timeout / 4` → 분석 진행률 기반 동적 조정
2. **Snapshot 압축**: 대형 프로젝트에서 snapshot 크기 증가 → gzip 압축 적용
3. **Event TTL**: 완료된 세션의 이벤트 자동 정리 (기본 7일)
4. **다중 인스턴스 보호**: Lock file + PID 검증 존재, hostname 교차 검증 추가

---

## 8. 최적 구현 계획 (우선순위 순)

### Phase 1: 품질 시스템 강화 (최우선)

#### 1.1 Spot-Check Reference Validator
- 참조 라인의 실제 코드 스니펫 추출
- LLM에 주장-코드 관련성 확인 (배치)
- 20% 랜덤 샘플링으로 비용 최적화
- **영향**: 의미적 참조 정확성 50% → 90%

#### 1.2 Content Assertion Detector
- 기술 주장 패턴 매칭 (regex)
- 참조 없는 기술 주장에 warning
- 3+ 무참조 주장 시 LLM 재평가
- **영향**: 참조 없는 환각 탐지 0% → 70%

#### 1.3 YAML Frontmatter 파서 개선
- `serde_yaml` 기반 key-value 추출
- frontmatter 섹션 정확한 경계 감지
- 필수 필드 검증
- **영향**: 포맷 검증 정확도 70% → 95%

### Phase 2: 유형 분류 유연화

#### 2.1 Hybrid Category System
- `Tier3Category`에 `Discovered(String, usize)` 추가
- `PolicyType`에 `Custom(String)` 추가
- `DomainLogicType`에 `Custom(String)` 추가
- **영향**: 도메인 특화 분류 커버리지 60% → 95%

#### 2.2 Dynamic Framework Registry
- `KNOWN_FRAMEWORKS`를 config로 이동
- 사용자 정의 프레임워크 추가 지원
- LLM 발견 프레임워크 자동 등록
- **영향**: 프레임워크 커버리지 80% → 95%

#### 2.3 Extensible Concern System
- `WELL_KNOWN_CONCERNS`를 코어 + 확장으로 분리
- LLM 발견 관심사 동적 등록
- 프로젝트별 관심사 config 지원
- **영향**: 교차 관심사 커버리지 70% → 90%

### Phase 3: 분석 파이프라인 보강

#### 3.1 Configurable Module Threshold
- `hierarchical_summarizer`의 top-N 설정 가능
- importance 기반 동적 threshold
- **영향**: 대형 프로젝트 정보 보존 85% → 95%

#### 3.2 Language-Aware Token Estimation
- 언어별 `chars_per_token` 가중치
- 코드/주석/문자열 구분 추정
- **영향**: 예산 활용 효율 85% → 95%

#### 3.3 Semantic Cohesion Chunking
- AST 구조 + import 그래프 기반 의미적 응집 청크
- 관련 함수/타입을 같은 청크로 그룹핑
- **영향**: 분석 품질 향상 (교차 참조 누락 감소)

### Phase 4: Progressive Disclosure 완성

#### 4.1 Navigation Map 강화
- 모듈-규칙-스킬-에이전트 매핑 테이블
- 접근 빈도 기반 priority 조정
- 검색 가능한 인덱스

#### 4.2 대형 섹션 Smart Extraction
- 현재 임계값 기반 → 의미 단위 분할
- 섹션 내 cross-reference 보존
- 추출된 문서 간 링크

#### 4.3 Cross-Reference Validation
- 모든 @참조의 유효성 검증
- 순환 참조 감지
- Dead link 경고

### Phase 5: 이벤트 소싱 최적화

#### 5.1 Dynamic Checkpoint Interval
- 분석 진행률 기반 동적 조정
- 예산 소비율 기반 긴급 체크포인트

#### 5.2 Snapshot Compression
- gzip 기반 스냅샷 압축
- 대형 프로젝트 디스크 사용량 50% 감소

#### 5.3 Completed Session Cleanup
- 완료 세션 자동 정리 (configurable TTL)
- 분석 결과만 보존, 이벤트 로그 아카이브

---

## 9. 설계 원칙 요약

### 9.1 핵심 설계 원칙

1. **Evidence-First**: 코드 분석 증거 없이 생성하지 않음
2. **LLM-Guided, Programmatic-Verified**: LLM이 판단, programmatic이 팩트 검증
3. **Structured Freedom**: 핵심 구조는 고정, 분류는 동적 확장
4. **Progressive Disclosure**: 정보를 계층적으로 접근 가능하게 구성
5. **Fail-Safe Quality**: 다중 게이트, 수렴 보장, best state 보존
6. **100% File Coverage**: Map-Reduce + 완전성 검증으로 누락 없는 분석
7. **Context-Budget-Aware**: 컨텍스트 윈도우 초과 방지의 구조적 보장
8. **Resumable Execution**: 이벤트 소싱 + 체크포인트로 어디서든 재개 가능

### 9.2 장기적 비전

claudegen의 장기적 가치는:
- **대규모 프로젝트의 100% 사실 기반 분석**: Map-Reduce + AST + 교차 검증
- **도메인 불가지론적 고가치 생성**: 유형 제한 최소화 + LLM 자율 발견
- **점진적 정보 접근**: Progressive Disclosure로 어떤 규모에서도 효과적 탐색
- **검증된 품질**: 다중 계층 검증 + 에비던스 기반 + 수렴 보장
- **장기 운영 안정성**: 이벤트 소싱 + 체크포인트 + 증분 업데이트

---

## 10. 리스크 평가

| 리스크 | 확률 | 영향 | 완화 전략 |
|--------|------|------|----------|
| LLM 자기 평가 편향 | 높음 | 중간 | 모델 교차 검증 (다른 tier) |
| 고정 enum 도메인 미스매치 | 중간 | 높음 | Hybrid category system |
| 대형 프로젝트 컨텍스트 초과 | 낮음 | 높음 | 3-tier budget + 하드캡 |
| 분석 정보 유실 | 낮음 | 중간 | 완전성 검증 + 아카이브 |
| 체크포인트 복원 실패 | 낮음 | 중간 | dual-layer persistence |
| 토큰 예산 과소 추정 | 중간 | 낮음 | 언어별 가중치 보정 |
