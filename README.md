# WeaveWiki

[![CI](https://github.com/junyeong-ai/weavewiki/workflows/CI/badge.svg)](https://github.com/junyeong-ai/weavewiki/actions)
[![Rust](https://img.shields.io/badge/rust-1.92.0%2B-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![DeepWiki](https://img.shields.io/badge/DeepWiki-junyeong--ai%2Fweavewiki-blue.svg?logo=data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAACwAAAAyCAYAAAAnWDnqAAAAAXNSR0IArs4c6QAAA05JREFUaEPtmUtyEzEQhtWTQyQLHNak2AB7ZnyXZMEjXMGeK/AIi+QuHrMnbChYY7MIh8g01fJoopFb0uhhEqqcbWTp06/uv1saEDv4O3n3dV60RfP947Mm9/SQc0ICFQgzfc4CYZoTPAswgSJCCUJUnAAoRHOAUOcATwbmVLWdGoH//PB8mnKqScAhsD0kYP3j/Yt5LPQe2KvcXmGvRHcDnpxfL2zOYJ1mFwrryWTz0advv1Ut4CJgf5uhDuDj5eUcAUoahrdY/56ebRWeraTjMt/00Sh3UDtjgHtQNHwcRGOC98BJEAEymycmYcWwOprTgcB6VZ5JK5TAJ+fXGLBm3FDAmn6oPPjR4rKCAoJCal2eAiQp2x0vxTPB3ALO2CRkwmDy5WohzBDwSEFKRwPbknEggCPB/imwrycgxX2NzoMCHhPkDwqYMr9tRcP5qNrMZHkVnOjRMWwLCcr8ohBVb1OMjxLwGCvjTikrsBOiA6fNyCrm8V1rP93iVPpwaE+gO0SsWmPiXB+jikdf6SizrT5qKasx5j8ABbHpFTx+vFXp9EnYQmLx02h1QTTrl6eDqxLnGjporxl3NL3agEvXdT0WmEost648sQOYAeJS9Q7bfUVoMGnjo4AZdUMQku50McDcMWcBPvr0SzbTAFDfvJqwLzgxwATnCgnp4wDl6Aa+Ax283gghmj+vj7feE2KBBRMW3FzOpLOADl0Isb5587h/U4gGvkt5v60Z1VLG8BhYjbzRwyQZemwAd6cCR5/XFWLYZRIMpX39AR0tjaGGiGzLVyhse5C9RKC6ai42ppWPKiBagOvaYk8lO7DajerabOZP46Lby5wKjw1HCRx7p9sVMOWGzb/vA1hwiWc6jm3MvQDTogQkiqIhJV0nBQBTU+3okKCFDy9WwferkHjtxib7t3xIUQtHxnIwtx4mpg26/HfwVNVDb4oI9RHmx5WGelRVlrtiw43zboCLaxv46AZeB3IlTkwouebTr1y2NjSpHz68WNFjHvupy3q8TFn3Hos2IAk4Ju5dCo8B3wP7VPr/FGaKiG+T+v+TQqIrOqMTL1VdWV1DdmcbO8KXBz6esmYWYKPwDL5b5FA1a0hwapHiom0r/cKaoqr+27/XcrS5UwSMbQAAAABJRU5ErkJggg==)](https://deepwiki.com/junyeong-ai/weavewiki)

> **[English](README.en.md)** | **한국어**

**AI가 코드베이스를 완벽하게 문서화합니다.** 100% 파일 커버리지, 100% 사실 기반 — 빠뜨리는 파일 없이, 추측 없이.

---

## 왜 WeaveWiki인가?

- **100% 커버리지** — 모든 소스 파일을 명시적으로 문서화
- **사실 기반** — 코드에서 관찰 가능한 사실만 기록
- **다중 에이전트** — 5단계 AI 파이프라인으로 깊이 있는 분석
- **중단 복구** — 언제든 중단하고 이어서 작업 가능

---

## 빠른 시작

```bash
# 설치
cargo install weavewiki

# 프로젝트 초기화 및 문서 생성
cd your-project
weavewiki init
weavewiki generate

# 결과 확인
ls .weavewiki/wiki/
```

---

## 주요 기능

### 문서 생성
```bash
weavewiki generate                    # 기본 분석
weavewiki generate --mode deep        # 심층 분석
weavewiki generate --resume           # 이전 세션 재개
weavewiki generate --status           # 진행 상태 확인
weavewiki generate --dry-run          # 설정만 확인
```

### 지식 그래프
```bash
weavewiki build                       # 코드 구조 분석
weavewiki query "src/main.rs"         # 의존성 조회
weavewiki validate                    # 문서-코드 정합성 검증
```

### 관리
```bash
weavewiki init                        # 프로젝트 초기화
weavewiki status                      # 상태 확인
weavewiki clean --all                 # 데이터 정리
weavewiki config show                 # 설정 확인
```

---

## 설치

### Cargo
```bash
cargo install weavewiki
```

### 소스 빌드
```bash
git clone https://github.com/junyeong-ai/weavewiki && cd weavewiki
cargo build --release
```

**요구사항**: Rust 1.92.0+

---

## LLM 프로바이더

### Claude Code (기본)
```bash
# API 키 불필요 - Claude Code CLI 사용
weavewiki generate
```

### OpenAI
```bash
export OPENAI_API_KEY="sk-..."
weavewiki generate --provider openai --model gpt-4o
```

### Ollama (로컬)
```bash
weavewiki generate --provider ollama --model llama3
```

---

## 분석 모드

| 모드 | 설명 | 용도 |
|------|------|------|
| `fast` | 빠른 개요 | 큰 프로젝트 미리보기 |
| `standard` | 균형잡힌 분석 | 일반 문서화 (기본값) |
| `deep` | 심층 분석 | 상세 문서 필요 시 |

```bash
weavewiki generate --mode deep --quality-target 0.9
```

---

## 설정

`.weavewiki/config.toml`:
```toml
[project]
name = "my-project"

[llm]
provider = "claude-code"
model = "claude-sonnet-4-20250514"

[analysis]
mode = "standard"
quality_target = 0.8
```

---

## 출력 구조

```
.weavewiki/wiki/
├── index.md              # 프로젝트 개요
├── llms.txt              # AI 에이전트용 컨텍스트
├── patterns.md           # 발견된 코드 패턴
├── constitution.md       # 코딩 컨벤션
└── domains/              # 도메인별 문서
    ├── core/
    ├── api/
    └── storage/
```

---

## 지원 언어

**파서 지원 (AST 분석)**: Rust, Go, Python, TypeScript, JavaScript, Java, Kotlin, C, C++, Ruby, Bash

**언어 감지**: 30+ 언어

---

## 문제 해결

```bash
# 데이터 초기화
weavewiki clean --all && weavewiki init

# 진행 상태 확인
weavewiki generate --status

# 디버그 모드
RUST_LOG=debug weavewiki generate
```

---

## 지원

- [GitHub Issues](https://github.com/junyeong-ai/weavewiki/issues)
- [개발자 가이드](CLAUDE.md)

---

<div align="center">

**[English](README.en.md)** | **한국어**

Made with Rust

</div>
