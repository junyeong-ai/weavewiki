# Map-Reduce 기반 고품질 분석 아키텍처 설계

## 1. 현재 파이프라인 심층 분석

### 1.1 현재 흐름

```
Files → ChunkingStrategy → DistributedAnalyzer → Aggregator → Synthesis
         (토큰 기반)         (병렬 LLM 호출)      (단순 병합)    (충돌 해결)
```

### 1.2 핵심 문제점

#### 문제 1: 구조 무시 청킹
```rust
// distributed.rs:154-182
impl ChunkingStrategy {
    pub fn create_chunks(registry, config) -> Vec<AnalysisChunk> {
        // 문제: 토큰 수만 보고 청킹
        if current_tokens + file.estimated_tokens > max_tokens {
            // 모듈 경계, 클래스 경계 무시
            // 관련 파일들이 다른 청크로 분산됨
        }
    }
}
```

**영향**:
- 관련 파일(예: `mod.rs`와 그 하위 모듈)이 분리됨
- 컨텍스트 손실로 의존성 분석 부정확

#### 문제 2: 대형 파일 정보 유실
```rust
// distributed.rs:429-445
if original_size > config.max_file_content_chars {
    // 단순 truncation - 중간 요약 없음
    truncated_files.push(TruncatedFile {
        truncation_ratio: analyzed_size as f32 / original_size as f32,
    });
    // 나머지 50%+ 내용 완전 유실
}
```

**영향**:
- 대형 파일의 후반부 패턴/제약조건 누락
- 파일 끝부분의 중요한 테스트/에러핸들링 코드 미분석

#### 문제 3: 중간 요약 부재
```rust
// aggregator.rs:147-161
pub fn aggregate(chunk_results) -> AggregatedAnalysis {
    // 단순 병합 - 계층적 요약 없음
    let conventions = Self::reduce_conventions(&chunk_results);
    let patterns = Self::merge_patterns(&chunk_results);
    // 청크 레벨 → 바로 전체 레벨
    // 모듈 레벨 요약 없음
}
```

**영향**:
- 모듈별 특성 파악 어려움
- 크로스 모듈 패턴 감지 미흡

#### 문제 4: 추가 조회 불가
```rust
// 현재: 분석 중 추가 파일 조회 메커니즘 없음
// LLM이 "이 파일 참조 필요" 신호를 보내도 무시됨
```

**영향**:
- 의존성 체인 추적 불완전
- 참조된 파일의 상세 내용 확인 불가

---

## 2. 개선된 Map-Reduce 아키텍처

### 2.1 전체 흐름

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    PHASE 0: AST 기반 구조 분석                               │
├─────────────────────────────────────────────────────────────────────────────┤
│  Files ──→ AST Parser ──→ StructureGraph                                    │
│                           ├── Modules (경계, 의존성)                         │
│                           ├── Types (클래스, 인터페이스)                      │
│                           └── Functions (진입점, 핵심 로직)                   │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                    PHASE 1: 스마트 청킹 (Map 준비)                           │
├─────────────────────────────────────────────────────────────────────────────┤
│  StructureGraph ──→ SmartChunker                                            │
│                     ├── 대형 파일 → FunctionChunks (함수 단위)               │
│                     ├── 중형 파일 → FileChunks (파일 전체)                   │
│                     └── 소형 파일 → ModuleChunks (모듈 그룹)                 │
│                                                                              │
│  [ChunkMetadata: parent_module, related_files, context_summary]             │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                    PHASE 2: 병렬 청크 분석 (Map)                             │
├─────────────────────────────────────────────────────────────────────────────┤
│  Chunks ──→ [Parallel LLM Analysis] ──→ ChunkResults                        │
│             │                                                                │
│             ├── 각 청크에 컨텍스트 주입 (모듈 구조, 관련 파일 요약)           │
│             ├── 추가 조회 요청 감지 → ReferenceResolver                      │
│             └── 중간 검증 (AST 기반 참조 검증)                               │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                    PHASE 3: 계층적 요약 (Reduce Layer 1)                     │
├─────────────────────────────────────────────────────────────────────────────┤
│  ChunkResults ──→ HierarchicalSummarizer                                    │
│                   │                                                          │
│                   ├── FunctionChunks → FileSummary (함수→파일 요약)         │
│                   ├── FileChunks → ModuleSummary (파일→모듈 요약)           │
│                   └── ModuleChunks → (이미 모듈 레벨)                        │
│                                                                              │
│  [ModuleSummary: patterns, constraints, gotchas, key_abstractions]          │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                    PHASE 4: 크로스 모듈 합성 (Reduce Layer 2)                │
├─────────────────────────────────────────────────────────────────────────────┤
│  ModuleSummaries ──→ CrossModuleSynthesizer                                 │
│                      ├── 패턴 병합 (중복 제거, 변형 추적)                    │
│                      ├── 제약조건 통합 (크로스 모듈 의존성)                  │
│                      ├── 숨겨진 의존성 감지                                   │
│                      └── 아키텍처 위반 탐지                                   │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                    PHASE 5: 완전성 검증 및 보완                              │
├─────────────────────────────────────────────────────────────────────────────┤
│  SynthesizedAnalysis ──→ CompletenessValidator                              │
│                          ├── Truncated 파일 재분석 (누락된 부분)             │
│                          ├── Low-confidence 영역 재분석                      │
│                          ├── 참조된 미분석 파일 추가 분석                     │
│                          └── 최종 검증 (100% 커버리지 확인)                   │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 2.2 핵심 컴포넌트 설계

#### 2.2.1 StructureGraph (AST 기반 구조)

```rust
pub struct StructureGraph {
    /// 모듈 계층 구조
    pub modules: HashMap<String, ModuleNode>,
    /// 타입 정의 맵
    pub types: HashMap<String, TypeNode>,
    /// 함수 맵 (파일:함수명 → 위치)
    pub functions: HashMap<String, FunctionNode>,
    /// 의존성 엣지
    pub dependencies: Vec<DependencyEdge>,
    /// 파일 크기 분류
    pub file_sizes: HashMap<String, FileSize>,
}

#[derive(Debug, Clone)]
pub struct ModuleNode {
    pub path: String,
    pub files: Vec<String>,
    pub public_exports: Vec<String>,
    pub internal_deps: Vec<String>,
    pub external_deps: Vec<String>,
    pub estimated_complexity: u32,
}

#[derive(Debug, Clone, Copy)]
pub enum FileSize {
    Small,      // < 200 lines
    Medium,     // 200-1000 lines
    Large,      // 1000-5000 lines
    VeryLarge,  // > 5000 lines
}
```

#### 2.2.2 SmartChunker (구조 기반 청킹)

```rust
pub struct SmartChunker {
    structure: StructureGraph,
    config: ChunkingConfig,
}

impl SmartChunker {
    pub fn create_chunks(&self) -> Vec<SmartChunk> {
        let mut chunks = Vec::new();

        for (path, size) in &self.structure.file_sizes {
            match size {
                FileSize::VeryLarge => {
                    // 함수 단위 분할 (AST 기반)
                    chunks.extend(self.chunk_by_functions(path));
                }
                FileSize::Large => {
                    // 섹션 단위 분할 (클래스, impl 블록)
                    chunks.extend(self.chunk_by_sections(path));
                }
                FileSize::Medium => {
                    // 파일 전체를 하나의 청크로
                    chunks.push(self.single_file_chunk(path));
                }
                FileSize::Small => {
                    // 같은 모듈의 소형 파일들 그룹화
                    // (별도 처리 - 모듈 그룹화 단계에서)
                }
            }
        }

        // 소형 파일 모듈 그룹화
        chunks.extend(self.group_small_files_by_module());

        // 각 청크에 컨텍스트 메타데이터 추가
        self.enrich_chunk_context(&mut chunks);

        chunks
    }

    fn chunk_by_functions(&self, path: &str) -> Vec<SmartChunk> {
        let functions = self.structure.functions_in(path);
        let mut chunks = Vec::new();
        let mut current_chunk = Vec::new();
        let mut current_lines = 0;

        for func in functions {
            if current_lines + func.line_count > self.config.max_lines_per_chunk
               && !current_chunk.is_empty() {
                chunks.push(SmartChunk::Functions {
                    file: path.to_string(),
                    functions: std::mem::take(&mut current_chunk),
                    context: self.get_file_context(path),
                });
                current_lines = 0;
            }
            current_chunk.push(func);
            current_lines += func.line_count;
        }

        if !current_chunk.is_empty() {
            chunks.push(SmartChunk::Functions {
                file: path.to_string(),
                functions: current_chunk,
                context: self.get_file_context(path),
            });
        }

        chunks
    }
}

pub enum SmartChunk {
    /// 대형 파일의 함수 단위 청크
    Functions {
        file: String,
        functions: Vec<FunctionRange>,
        context: FileContext,
    },
    /// 중형 파일 전체
    SingleFile {
        file: String,
        context: FileContext,
    },
    /// 소형 파일 모듈 그룹
    ModuleGroup {
        module_path: String,
        files: Vec<String>,
        context: ModuleContext,
    },
}

pub struct FileContext {
    /// 파일이 속한 모듈
    pub module: String,
    /// 파일 내 타입 목록 (요약)
    pub types_summary: String,
    /// 주요 import 문
    pub imports: Vec<String>,
    /// 관련 파일 (같은 모듈)
    pub related_files: Vec<String>,
}
```

#### 2.2.3 HierarchicalSummarizer (계층적 요약)

```rust
pub struct HierarchicalSummarizer {
    provider: Arc<dyn LlmProvider>,
}

impl HierarchicalSummarizer {
    /// 청크 결과 → 파일 요약 → 모듈 요약
    pub async fn summarize(
        &self,
        chunk_results: Vec<ChunkAnalysisResult>,
        structure: &StructureGraph,
    ) -> Vec<ModuleSummary> {
        // Step 1: 함수 청크 → 파일 요약
        let file_summaries = self.aggregate_to_files(chunk_results, structure).await;

        // Step 2: 파일 요약 → 모듈 요약
        let module_summaries = self.aggregate_to_modules(file_summaries, structure).await;

        module_summaries
    }

    async fn aggregate_to_files(
        &self,
        chunk_results: Vec<ChunkAnalysisResult>,
        structure: &StructureGraph,
    ) -> HashMap<String, FileSummary> {
        let mut file_chunks: HashMap<String, Vec<ChunkAnalysisResult>> = HashMap::new();

        // 같은 파일의 청크들 그룹화
        for result in chunk_results {
            if let Some(file) = result.source_file() {
                file_chunks.entry(file).or_default().push(result);
            }
        }

        let mut file_summaries = HashMap::new();

        for (file, chunks) in file_chunks {
            if chunks.len() == 1 {
                // 단일 청크 → 바로 요약
                file_summaries.insert(file.clone(), FileSummary::from_chunk(&chunks[0]));
            } else {
                // 다중 청크 → LLM 요약
                let summary = self.summarize_file_chunks(&file, &chunks).await;
                file_summaries.insert(file, summary);
            }
        }

        file_summaries
    }

    async fn summarize_file_chunks(
        &self,
        file: &str,
        chunks: &[ChunkAnalysisResult],
    ) -> FileSummary {
        let prompt = format!(
            r#"Summarize findings from multiple analysis chunks of file: {}

Chunks:
{}

Create a unified file summary that:
1. Merges patterns (deduplicate, note variations)
2. Combines constraints (resolve conflicts)
3. Lists all gotchas (preserve all)
4. Identifies key abstractions

Return JSON: {{
  "patterns": [...],
  "constraints": [...],
  "gotchas": [...],
  "key_abstractions": [...],
  "file_purpose": "..."
}}"#,
            file,
            chunks.iter().map(|c| c.to_summary_string()).collect::<Vec<_>>().join("\n---\n")
        );

        // LLM 호출하여 요약
        // ...
    }
}
```

#### 2.2.4 ReferenceResolver (추가 조회)

```rust
pub struct ReferenceResolver {
    file_registry: VerifiedFileRegistry,
    file_cache: HashMap<String, FileContent>,
}

impl ReferenceResolver {
    /// 분석 중 추가 파일 조회 요청 처리
    pub async fn resolve_references(
        &mut self,
        requests: Vec<ReferenceRequest>,
    ) -> Vec<ResolvedReference> {
        let mut results = Vec::new();

        for request in requests {
            let content = self.get_file_content(&request.file_path).await;

            let resolved = match request.scope {
                ReferenceScope::FullFile => {
                    ResolvedReference::FullFile {
                        path: request.file_path,
                        content,
                    }
                }
                ReferenceScope::Function(name) => {
                    let func_content = self.extract_function(&content, &name);
                    ResolvedReference::Function {
                        path: request.file_path,
                        name,
                        content: func_content,
                    }
                }
                ReferenceScope::Lines(start, end) => {
                    let lines = self.extract_lines(&content, start, end);
                    ResolvedReference::Lines {
                        path: request.file_path,
                        start,
                        end,
                        content: lines,
                    }
                }
            };

            results.push(resolved);
        }

        results
    }
}

pub struct ReferenceRequest {
    pub file_path: String,
    pub scope: ReferenceScope,
    pub reason: String,
}

pub enum ReferenceScope {
    FullFile,
    Function(String),
    Lines(u32, u32),
}
```

#### 2.2.5 CompletenessValidator (완전성 검증)

```rust
pub struct CompletenessValidator {
    structure: StructureGraph,
    analyzer: DistributedAnalyzer,
}

impl CompletenessValidator {
    pub async fn validate_and_complete(
        &self,
        analysis: &mut SynthesizedAnalysis,
        coverage: &Coverage,
    ) -> Result<CompletenessReport> {
        let mut report = CompletenessReport::default();

        // 1. Truncated 파일 재분석
        for truncated in &coverage.truncated_files {
            if truncated.truncation_ratio < 0.7 {
                // 30% 이상 손실 → 나머지 부분 재분석
                let remaining = self.analyze_remaining(
                    &truncated.path,
                    truncated.analyzed_size,
                ).await?;

                self.merge_remaining_analysis(analysis, remaining);
                report.reanalyzed_files.push(truncated.path.clone());
            }
        }

        // 2. Failed 파일 재시도
        for failed in &coverage.failed_files {
            if let Ok(result) = self.retry_file_analysis(&failed.path).await {
                self.merge_file_analysis(analysis, result);
                report.recovered_files.push(failed.path.clone());
            }
        }

        // 3. 참조된 미분석 파일 확인
        let referenced_files = self.extract_referenced_files(analysis);
        let unanalyzed: Vec<_> = referenced_files
            .iter()
            .filter(|f| !coverage.analyzed_file(f))
            .collect();

        for file in unanalyzed {
            let result = self.analyze_single_file(file).await?;
            self.merge_file_analysis(analysis, result);
            report.additional_files.push(file.clone());
        }

        // 4. Low-confidence 영역 재분석
        let low_confidence = analysis.find_low_confidence_areas();
        for area in low_confidence {
            let enhanced = self.deep_analyze_area(&area).await?;
            self.enhance_analysis(analysis, area, enhanced);
            report.enhanced_areas.push(area.description.clone());
        }

        // 5. 최종 커버리지 확인
        report.final_coverage = self.calculate_final_coverage(analysis);

        Ok(report)
    }

    async fn analyze_remaining(
        &self,
        file: &str,
        already_analyzed: usize,
    ) -> Result<ChunkAnalysisResult> {
        // 파일의 나머지 부분 읽기
        let content = tokio::fs::read_to_string(file).await?;
        let remaining_content = &content[already_analyzed..];

        // 나머지 부분만 분석
        // ...
    }
}

#[derive(Debug, Default)]
pub struct CompletenessReport {
    pub reanalyzed_files: Vec<String>,
    pub recovered_files: Vec<String>,
    pub additional_files: Vec<String>,
    pub enhanced_areas: Vec<String>,
    pub final_coverage: f32,
}
```

---

## 3. 구현 우선순위

### Phase A: 기반 구축 (필수)
1. `StructureGraph` 구현 - AST 기반 전체 구조 파악
2. `SmartChunker` 구현 - 구조 기반 청킹
3. `ChunkContext` 강화 - 각 청크에 관련 컨텍스트 주입

### Phase B: 요약 계층 (핵심)
4. `HierarchicalSummarizer` 구현 - 함수→파일→모듈 요약
5. `ModuleSummary` 타입 정의 - 모듈 레벨 분석 결과
6. Aggregator 개선 - 계층적 병합

### Phase C: 완전성 보장 (고품질)
7. `ReferenceResolver` 구현 - 추가 파일 조회
8. `CompletenessValidator` 구현 - 유실 복구
9. 재분석 메커니즘 - truncated/failed 파일 처리

### Phase D: 통합 및 최적화
10. `AdaptivePipeline` 통합
11. 병렬 처리 최적화
12. 캐싱 레이어 추가

---

## 4. 예상 효과

| 지표 | 현재 | 개선 후 |
|------|------|---------|
| 파일 커버리지 | ~95% | 100% |
| 대형 파일 분석 | 50-70% | 100% |
| 정보 유실 | 있음 | 없음 |
| 모듈 레벨 인사이트 | 제한적 | 완전 |
| 크로스 모듈 패턴 | 부분 감지 | 완전 감지 |
| 추가 컨텍스트 | 불가 | 가능 |

---

## 5. 데이터 흐름 상세

```
                         ┌──────────────────┐
                         │   Source Files   │
                         └────────┬─────────┘
                                  │
                    ┌─────────────▼─────────────┐
                    │     AST Parser            │
                    │  (tree-sitter 기반)       │
                    └─────────────┬─────────────┘
                                  │
                    ┌─────────────▼─────────────┐
                    │    StructureGraph         │
                    │  ├── ModuleTree           │
                    │  ├── TypeRegistry         │
                    │  ├── FunctionMap          │
                    │  └── DependencyGraph      │
                    └─────────────┬─────────────┘
                                  │
                    ┌─────────────▼─────────────┐
                    │     SmartChunker          │
                    │  ├── VeryLarge → Funcs    │
                    │  ├── Large → Sections     │
                    │  ├── Medium → Files       │
                    │  └── Small → Modules      │
                    └─────────────┬─────────────┘
                                  │
              ┌───────────────────┼───────────────────┐
              │                   │                   │
              ▼                   ▼                   ▼
       ┌──────────┐        ┌──────────┐        ┌──────────┐
       │ Chunk 1  │        │ Chunk 2  │   ...  │ Chunk N  │
       │ +Context │        │ +Context │        │ +Context │
       └────┬─────┘        └────┬─────┘        └────┬─────┘
            │                   │                   │
            │     [Parallel LLM Analysis]          │
            │                   │                   │
            ▼                   ▼                   ▼
       ┌──────────┐        ┌──────────┐        ┌──────────┐
       │ Result 1 │        │ Result 2 │   ...  │ Result N │
       └────┬─────┘        └────┬─────┘        └────┬─────┘
            │                   │                   │
            └───────────────────┼───────────────────┘
                                │
                    ┌───────────▼───────────┐
                    │  HierarchicalSummarizer│
                    │  ├── → FileSummary     │
                    │  └── → ModuleSummary   │
                    └───────────┬───────────┘
                                │
                    ┌───────────▼───────────┐
                    │ CrossModuleSynthesizer │
                    │  ├── Pattern Merge     │
                    │  ├── Constraint Union  │
                    │  └── Dependency Detect │
                    └───────────┬───────────┘
                                │
                    ┌───────────▼───────────┐
                    │ CompletenessValidator  │
                    │  ├── Truncated → Redo  │
                    │  ├── Failed → Retry    │
                    │  └── 100% Coverage     │
                    └───────────┬───────────┘
                                │
                    ┌───────────▼───────────┐
                    │  SynthesizedAnalysis  │
                    │  (고품질, 완전성 보장) │
                    └───────────────────────┘
```

이 설계는 맵-리듀스 패턴을 완전히 활용하여 정보 유실 없이 고품질 분석을 보장합니다.
