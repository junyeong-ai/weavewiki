# claudegen 재설계 작업 목록

> **Version**: 2.0
> **Date**: 2025-01-20
> **Status**: Planning

---

## 작업 상태 범례

- `[ ]` 미시작
- `[~]` 진행 중
- `[x]` 완료
- `[!]` 차단됨

---

## Phase 1: Foundation (예상: 1-2주)

### 1.1 설정 체계 재구성

#### 1.1.1 config/types.rs 재설계 [P0]

```
[ ] 새 설정 구조 정의
    [ ] GenerationConfig 섹션
        [ ] strategy: ValueDriven | CoverageDriven | Minimal
        [ ] artifacts: Vec<ArtifactType>
        [ ] limits: ArtifactLimits

    [ ] ValueConfig 섹션
        [ ] min_overall: f32
        [ ] dimensions: ValueDimensions
        [ ] weights: ValueWeights

    [ ] ConvergenceConfig 섹션
        [ ] consecutive_passes: usize
        [ ] max_oscillations: usize
        [ ] max_iterations: usize
        [ ] early_exit_threshold: f32
        [ ] stagnation_patience: usize

    [ ] DomainConfig 섹션
        [ ] domain_type: DomainType enum
        [ ] terminology: Vec<TermDefinition>
        [ ] compliance: Vec<String>

    [ ] TierConfig 섹션
        [ ] tier0: TierPatterns
        [ ] tier3: TierPatterns
        [ ] custom_patterns: Vec<CustomPattern>

    [ ] ArtifactConfigs
        [ ] rules: RulesConfig
        [ ] skills: SkillsConfig
        [ ] agents: AgentsConfig
        [ ] claude_md: ClaudeMdConfig

    [ ] 검증 로직 업데이트
        [ ] validate() 메서드 확장
        [ ] 크로스 필드 검증 추가
        [ ] 에러 메시지 개선
```

#### 1.1.2 config/loader.rs 개선 [P0]

```
[ ] 프리셋 자동 적용 로직
    [ ] ConfigLoader::load()에서 프리셋 감지
    [ ] Preset::apply() 호출 연결
    [ ] 프리셋 후 파일 오버라이드 순서 보장

[ ] 도메인 프리셋 지원
    [ ] DomainPreset enum 정의
        [ ] ECommerce
        [ ] FinTech
        [ ] Healthcare
        [ ] SaaS
        [ ] Generic
    [ ] 도메인별 기본 Tier 패턴
    [ ] 도메인별 기본 규정 요구사항

[ ] 환경변수 오버라이드 확장
    [ ] 복잡 타입 지원 (JSON 문자열 파싱)
    [ ] 배열 타입 지원
    [ ] 환경변수 문서화
```

#### 1.1.3 CLI 확장 [P1]

```
[ ] main.rs 옵션 추가
    [ ] --preset <quick|standard|thorough|exhaustive>
    [ ] --domain <ecommerce|fintech|healthcare|saas|generic>
    [ ] --min-value <f32>
    [ ] --min-mistake-prevention <f32>
    [ ] --max-iterations <usize>
    [ ] --consecutive-passes <usize>
    [ ] --model <model-id>
    [ ] --insight-model <model-id>
    [ ] --depth <shallow|moderate|deep|exhaustive>
    [ ] --budget-tokens <u64>
    [ ] --dry-run (설정만 표시)

[ ] generate.rs 옵션 처리
    [ ] GenerateOptions 구조체 확장
    [ ] 옵션 → Config 오버라이드 적용
    [ ] 검증 및 에러 처리
```

### 1.2 수렴 시스템 구현

#### 1.2.1 convergence/criteria.rs 신규 [P0]

```
[ ] ConvergenceCriteria 구조체
    [ ] formal: FormalCriteria
    [ ] value: ValueCriteria
    [ ] cross_artifact: CrossArtifactCriteria
    [ ] stability: StabilityCriteria

[ ] FormalCriteria
    [ ] all_references_valid: bool
    [ ] all_structures_valid: bool
    [ ] no_tier0_content: bool
    [ ] is_satisfied() 메서드

[ ] ValueCriteria
    [ ] min_mistake_prevention: f32
    [ ] min_discoverability: f32
    [ ] min_artifact_fitness: f32
    [ ] min_overall_value: f32
    [ ] is_satisfied() 메서드

[ ] CrossArtifactCriteria
    [ ] no_redundancy: bool
    [ ] no_contradiction: bool
    [ ] all_references_consistent: bool
    [ ] min_coverage: f32
    [ ] is_satisfied() 메서드

[ ] StabilityCriteria
    [ ] consecutive_passes: usize
    [ ] max_oscillations: usize
    [ ] current_streak: usize
    [ ] oscillation_count: usize
    [ ] is_satisfied() 메서드

[ ] is_converged() → ConvergenceStatus
    [ ] ConvergenceStatus enum
        [ ] Converged
        [ ] NeedRegeneration(Vec<Issue>)
        [ ] NeedRefinement(Vec<Suggestion>)
        [ ] NeedAdjustment(Vec<Issue>)
        [ ] NeedStabilization
```

#### 1.2.2 convergence/loop.rs 신규 [P0]

```
[ ] ConvergenceLoop 구조체
    [ ] config: ConvergenceConfig
    [ ] criteria: ConvergenceCriteria
    [ ] history: Vec<ValidationResult>

[ ] run() 메서드
    [ ] 수렴 조건 체크 루프
    [ ] 조건별 분기 처리
    [ ] 진행 이력 기록
    [ ] 조기 종료 처리
    [ ] stagnation 감지

[ ] 단위 테스트
    [ ] 정상 수렴 케이스
    [ ] 형식 검증 실패 케이스
    [ ] 가치 검증 실패 케이스
    [ ] stagnation 케이스
    [ ] oscillation 케이스
```

### 1.3 검증 시스템 리팩토링

#### 1.3.1 validation/formal/ 리팩토링 [P1]

```
[ ] reference.rs
    [ ] 파일 경로 존재 확인
    [ ] 라인 번호 범위 유효성
    [ ] URL 접근 가능 여부 (옵션)
    [ ] 참조 타입별 검증 로직

[ ] structure.rs
    [ ] YAML 구문 검증
    [ ] JSON 구문 검증
    [ ] Markdown 구조 검증
    [ ] 필수 필드 검증
    [ ] 타입 정합성 검증

[ ] tier_filter.rs
    [ ] Tier0 키워드 매칭
    [ ] Tier0 패턴 매칭
    [ ] 커스텀 패턴 지원
    [ ] 설정 기반 패턴 로드
```

---

## Phase 2: Insight Engine (예상: 2-3주)

### 2.1 실수 발견 엔진

#### 2.1.1 insight/mistake_finder.rs 신규 [P0]

```
[ ] MistakeFinder 구조체
    [ ] provider: LlmProvider
    [ ] project_analysis: ProjectAnalysis

[ ] find_potential_mistakes() 메서드
    [ ] 기술적 실수 시나리오
        [ ] 동시성 관련 실수
        [ ] 초기화 순서 실수
        [ ] 리소스 관리 실수
        [ ] 에러 처리 누락
    [ ] 비즈니스 로직 실수
        [ ] 규칙 적용 누락
        [ ] 조건 체크 누락
        [ ] 상태 전이 오류
    [ ] 도메인 특화 실수
        [ ] 규정 준수 누락
        [ ] 용어 오용
        [ ] 프로세스 위반

[ ] evaluate_severity() 메서드
    [ ] 영향 범위 평가
    [ ] 발생 가능성 평가
    [ ] 복구 난이도 평가
    [ ] 종합 심각도 점수

[ ] derive_prevention_info() 메서드
    [ ] 실수 → 방지 정보 매핑
    [ ] 증거 파일 연결
    [ ] Artifact 유형 추천
```

#### 2.1.2 LLM 프롬프트 템플릿

```
[ ] prompts/mistake_discovery.yaml
    [ ] 시스템 프롬프트
    [ ] 분석 컨텍스트 템플릿
    [ ] 출력 스키마 정의
    [ ] Few-shot 예시

[ ] prompts/severity_evaluation.yaml
    [ ] 심각도 평가 기준
    [ ] 평가 프롬프트
    [ ] 출력 스키마
```

### 2.2 제약 탐지기

#### 2.2.1 insight/constraint_detector.rs 신규 [P0]

```
[ ] ConstraintDetector 구조체
    [ ] analyzers: Vec<Box<dyn ConstraintAnalyzer>>

[ ] ConstraintAnalyzer trait
    [ ] detect() → Vec<Constraint>
    [ ] name() → &str

[ ] 동시성 제약 분석기
    [ ] Arc/Rc 공유 패턴
    [ ] Mutex/RwLock 사용
    [ ] Channel 통신
    [ ] atomic 연산

[ ] 초기화 순서 분석기
    [ ] 의존성 그래프 구축
    [ ] 순서 제약 추출
    [ ] lazy 초기화 패턴

[ ] 보안 제약 분석기
    [ ] 인증 체크 패턴
    [ ] 암호화 사용 패턴
    [ ] 입력 검증 패턴
    [ ] 권한 체크 패턴

[ ] 경계 조건 분석기
    [ ] 범위 체크 패턴
    [ ] 크기 제한 패턴
    [ ] 타임아웃 설정
    [ ] 재시도 제한
```

### 2.3 도메인 분석기

#### 2.3.1 insight/domain_analyzer.rs 신규 [P0]

```
[ ] DomainAnalyzer 구조체
    [ ] domain_type: DomainType
    [ ] terminology: TerminologyExtractor
    [ ] rule_extractor: BusinessRuleExtractor

[ ] analyze() 메서드
    [ ] 비즈니스 엔티티 식별
    [ ] 도메인 용어 추출
    [ ] 비즈니스 규칙 추출
    [ ] 규정 요구사항 파악

[ ] TerminologyExtractor
    [ ] 코드 주석에서 추출
    [ ] 변수/함수명에서 추출
    [ ] 문서에서 추출
    [ ] LLM 보조 추출

[ ] BusinessRuleExtractor
    [ ] 조건문 분석
    [ ] 검증 로직 분석
    [ ] 상태 전이 분석
    [ ] LLM 보조 추출
```

### 2.4 지식 분류기

#### 2.4.1 insight/knowledge_classifier.rs 신규 [P1]

```
[ ] KnowledgeClassifier 구조체
    [ ] tier_patterns: TierConfig
    [ ] artifact_rules: ArtifactClassificationRules

[ ] classify_tier() 메서드
    [ ] Tier0 (거부) 판정
    [ ] Tier1 (낮은 가치) 판정
    [ ] Tier2 (중간 가치) 판정
    [ ] Tier3 (높은 가치) 판정

[ ] classify_artifact() 메서드
    [ ] CLAUDE.md 적합성
    [ ] Rules 적합성
    [ ] Skills 적합성
    [ ] Agents 적합성

[ ] detect_duplicates() 메서드
    [ ] 의미적 유사도 계산
    [ ] 중복 그룹화
    [ ] 병합 제안
```

---

## Phase 3: Value-Driven Generation (예상: 2-3주)

### 3.1 Artifact 생성기

#### 3.1.1 generation/claude_md.rs 신규 [P0]

```
[ ] ClaudeMdGenerator 구조체
    [ ] provider: LlmProvider
    [ ] config: ClaudeMdConfig
    [ ] insights: Vec<Insight>

[ ] generate() 메서드
    [ ] 섹션별 생성
        [ ] Core Abstraction
        [ ] Critical Constraints
        [ ] Architecture Intent
        [ ] Domain Context
        [ ] Gotchas
        [ ] Extension Points
    [ ] 섹션 통합 및 구조화
    [ ] 참조 연결

[ ] 섹션별 프롬프트 템플릿
    [ ] prompts/claude_md/core_abstraction.yaml
    [ ] prompts/claude_md/critical_constraints.yaml
    [ ] prompts/claude_md/architecture_intent.yaml
    [ ] prompts/claude_md/domain_context.yaml
    [ ] prompts/claude_md/gotchas.yaml
    [ ] prompts/claude_md/extension_points.yaml
```

#### 3.1.2 generation/rules.rs 신규 [P0]

```
[ ] RulesGenerator 구조체
    [ ] provider: LlmProvider
    [ ] config: RulesConfig
    [ ] constraints: Vec<Constraint>
    [ ] business_rules: Vec<BusinessRule>

[ ] generate() 메서드
    [ ] 제약 → Rule 변환
    [ ] 유형별 생성
        [ ] Technical Rules
        [ ] Business Rules
        [ ] Security Rules
        [ ] Compliance Rules
    [ ] 중복 제거
    [ ] 우선순위 정렬

[ ] Rule 구조
    [ ] name: String
    [ ] rule_type: RuleType
    [ ] constraint: ConstraintType
    [ ] what: String
    [ ] why: String
    [ ] evidence: Vec<Evidence>
    [ ] example: Option<Example>
```

#### 3.1.3 generation/skills.rs 신규 [P0]

```
[ ] SkillsGenerator 구조체
    [ ] provider: LlmProvider
    [ ] config: SkillsConfig
    [ ] patterns: Vec<TaskPattern>

[ ] generate() 메서드
    [ ] 확장 포인트 → Skill 변환
    [ ] 반복 작업 → Skill 변환
    [ ] 구조화
        [ ] Context 섹션
        [ ] Task 섹션
        [ ] Verification 섹션
        [ ] References 섹션

[ ] Skill 구조
    [ ] name: String
    [ ] description: String
    [ ] when_to_use: String
    [ ] prompt: SkillPrompt
```

#### 3.1.4 generation/agents.rs 신규 [P1]

```
[ ] AgentsGenerator 구조체
    [ ] provider: LlmProvider
    [ ] config: AgentsConfig
    [ ] domains: Vec<DomainArea>

[ ] generate() 메서드
    [ ] 도메인 영역 → Agent 변환
    [ ] 역할 정의
    [ ] 도구 할당
    [ ] 프롬프트 생성

[ ] Agent 구조
    [ ] name: String
    [ ] role: String
    [ ] model: AgentModel
    [ ] tools: Vec<Tool>
    [ ] prompt: String
```

### 3.2 가치 검증

#### 3.2.1 validation/value/mistake_prevention.rs 신규 [P0]

```
[ ] MistakePreventionValidator 구조체
    [ ] provider: LlmProvider
    [ ] config: ValueConfig

[ ] validate() 메서드
    [ ] 실수 시나리오 생성 요청
    [ ] 심각도 평가
    [ ] 점수 계산

[ ] prompts/value/mistake_prevention.yaml
    [ ] 검증 질문 템플릿
    [ ] 평가 기준
    [ ] 출력 스키마
```

#### 3.2.2 validation/value/discoverability.rs 신규 [P0]

```
[ ] DiscoverabilityValidator 구조체
    [ ] provider: LlmProvider
    [ ] config: ValueConfig

[ ] validate() 메서드
    [ ] 발견 가능성 평가 요청
    [ ] 소스 분류 (코드/주석/경험)
    [ ] 점수 계산

[ ] prompts/value/discoverability.yaml
```

#### 3.2.3 validation/value/artifact_fitness.rs 신규 [P0]

```
[ ] ArtifactFitnessValidator 구조체
    [ ] provider: LlmProvider
    [ ] config: ValueConfig

[ ] validate() 메서드
    [ ] Artifact 유형별 기준 평가
    [ ] 적합성 점수 계산

[ ] 유형별 기준
    [ ] Rules: 제약성, 명확성, 검증가능성
    [ ] Skills: 재사용성, 단계성, 검증체크리스트
    [ ] Agents: 역할명확성, 도메인지식, 도구적절성
    [ ] CLAUDE.md: 맥락제공, 제약명시, 함정설명
```

---

## Phase 4: Refinement & Learning (예상: 1-2주)

### 4.1 이슈 진단

#### 4.1.1 refinement/issue_diagnosis.rs 신규 [P1]

```
[ ] IssueDiagnoser 구조체
    [ ] provider: LlmProvider
    [ ] learning: LearningSystem

[ ] diagnose() 메서드
    [ ] 형식 검증 실패 분석
    [ ] 가치 검증 실패 분석
    [ ] 교차 검증 실패 분석
    [ ] 근본 원인 추론

[ ] DiagnosisResult 구조체
    [ ] root_cause: RootCause
    [ ] affected_artifacts: Vec<ArtifactId>
    [ ] suggested_actions: Vec<Action>
    [ ] regenerate_scope: RegenerateScope
```

### 4.2 Targeted Fix

#### 4.2.1 refinement/targeted_fix.rs 신규 [P1]

```
[ ] TargetedFixer 구조체
    [ ] provider: LlmProvider
    [ ] generators: ArtifactGenerators

[ ] fix() 메서드
    [ ] 진단 결과 기반 수정 범위 결정
    [ ] 부분 재생성 vs 전체 재생성
    [ ] 수정 적용
    [ ] 재검증

[ ] RegenerateScope enum
    [ ] Section(artifact_id, section_name)
    [ ] Artifact(artifact_id)
    [ ] ArtifactType(artifact_type)
    [ ] All
```

### 4.3 학습 시스템 통합

#### 4.3.1 learning.rs 통합 [P1]

```
[ ] refinement 루프에서 호출 연결
    [ ] should_skip_strategy() 호출
    [ ] record_failure() 호출
    [ ] record_success() 호출

[ ] 패턴 저장/로드
    [ ] 세션 종료 시 저장
    [ ] 세션 시작 시 로드
    [ ] 오래된 패턴 정리
```

---

## Phase 5: Integration (예상: 1주)

### 5.1 파이프라인 통합

#### 5.1.1 pipeline/orchestrator.rs 신규 [P0]

```
[ ] PipelineOrchestrator 구조체
    [ ] config: Config
    [ ] provider: LlmProvider
    [ ] phases: Vec<Phase>

[ ] run() 메서드
    [ ] Phase 1: Deep Understanding 실행
    [ ] Phase 2: Insight Extraction 실행
    [ ] Phase 3: Artifact Generation 실행
    [ ] Phase 4: Value Validation 실행
    [ ] Phase 5: Convergence Loop 실행

[ ] 단계 간 데이터 전달
    [ ] ProjectAnalysis
    [ ] InsightList
    [ ] ArtifactDrafts
    [ ] ValidationResults
```

### 5.2 Progress 활성화

#### 5.2.1 cli/progress.rs 활성화 [P2]

```
[ ] generate.rs에서 ProgressTracker 생성
[ ] ConsoleRenderer 연결
[ ] 단계별 진행상황 업데이트
    [ ] Phase 표시
    [ ] 현재 작업 표시
    [ ] 진행률 표시
    [ ] ETA 표시
```

### 5.3 테스트

#### 5.3.1 통합 테스트 [P0]

```
[ ] 언어별 테스트
    [ ] Rust 프로젝트
    [ ] Python 프로젝트
    [ ] TypeScript 프로젝트
    [ ] Java 프로젝트
    [ ] Go 프로젝트

[ ] 도메인별 테스트
    [ ] E-Commerce 샘플
    [ ] FinTech 샘플
    [ ] Healthcare 샘플
    [ ] SaaS 샘플

[ ] 프리셋별 테스트
    [ ] quick 프리셋
    [ ] standard 프리셋
    [ ] thorough 프리셋
```

### 5.4 문서화

#### 5.4.1 사용자 문서 [P1]

```
[ ] docs/USER_GUIDE.md
    [ ] 설치 및 시작
    [ ] 기본 사용법
    [ ] 설정 가이드
    [ ] 프리셋 설명
    [ ] CLI 레퍼런스

[ ] docs/CONFIG_REFERENCE.md
    [ ] 전체 설정 항목
    [ ] 기본값
    [ ] 예시

[ ] docs/EXAMPLES.md
    [ ] 다양한 프로젝트 예시
    [ ] 도메인별 예시
```

---

## 의존성 그래프

```
Phase 1 ─┬─ 1.1 Config ──────────────────────┐
         │                                    │
         ├─ 1.2 Convergence ─────────────────┼─── Phase 3
         │                                    │
         └─ 1.3 Formal Validation ───────────┘

Phase 2 ─┬─ 2.1 Mistake Finder ──────────────┐
         │                                    │
         ├─ 2.2 Constraint Detector ─────────┼─── Phase 3
         │                                    │
         ├─ 2.3 Domain Analyzer ─────────────┤
         │                                    │
         └─ 2.4 Knowledge Classifier ────────┘

Phase 3 ─┬─ 3.1 Artifact Generators ─────────┐
         │                                    │
         └─ 3.2 Value Validation ────────────┼─── Phase 4 ─── Phase 5

Phase 4 ─┬─ 4.1 Issue Diagnosis ─────────────┤
         │                                    │
         ├─ 4.2 Targeted Fix ────────────────┤
         │                                    │
         └─ 4.3 Learning Integration ────────┘
```

---

## 마일스톤

| 마일스톤 | 예상 완료 | 핵심 산출물 |
|---------|----------|------------|
| M1: Foundation | Week 2 | 새 Config + 수렴 시스템 |
| M2: Insight Engine | Week 4-5 | 통찰 추출 파이프라인 |
| M3: Generation | Week 7 | 가치 기반 Artifact 생성 |
| M4: Refinement | Week 8-9 | 진단 기반 개선 |
| M5: Release | Week 10 | 통합 + 문서화 + 테스트 |

---

## 리스크 및 대응

| 리스크 | 가능성 | 영향 | 대응 |
|--------|--------|------|------|
| LLM 비용 초과 | 중 | 높음 | 캐싱, 프롬프트 최적화, 단계별 검증 |
| 검증 루프 무한 | 낮음 | 높음 | max_iterations 안전장치, oscillation 감지 |
| 도메인 특화 부족 | 중 | 중 | 도메인 프리셋 확장, 커스터마이징 지원 |
| 기존 코드 충돌 | 높음 | 중 | 점진적 마이그레이션, 호환성 레이어 |
