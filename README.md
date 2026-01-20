# claudegen

[![CI](https://github.com/junyeong-ai/claudegen/workflows/CI/badge.svg)](https://github.com/junyeong-ai/claudegen/actions)
[![Rust](https://img.shields.io/badge/rust-1.92.0%2B-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)

> **[English](README.en.md)** | **한국어**

**코드베이스 분석을 통한 Claude Code 플러그인 자동 생성.** 공식 Claude Code 플러그인 아키텍처 기반.

---

## 왜 claudegen인가?

- **공식 플러그인 형식** — Claude Code 공식 구조 준수
- **자동 Skills 생성** — 프로젝트별 맞춤 skills 자동 생성
- **메모리 관리** — CLAUDE.md에 프로젝트 규칙과 가이드라인 포함
- **확장 가능** — 커스텀 에이전트와 훅 지원

---

## 출력 구조 (공식 Claude Code 플러그인)

```
project/
├── CLAUDE.md                          # 프로젝트 메모리 (규칙, 가이드라인)
└── .claudegen/                        # 플러그인 디렉토리
    ├── .claude-plugin/
    │   └── plugin.json                # 플러그인 매니페스트
    ├── skills/
    │   └── {skill-name}/
    │       └── SKILL.md               # YAML 프론트매터 포함 스킬
    └── agents/
        └── {agent-name}.md            # 에이전트 정의
```

---

## 빠른 시작

```bash
# 설치
cargo install claudegen

# 프로젝트 초기화 및 플러그인 생성
cd your-project
claudegen init
claudegen generate

# 결과 확인
ls .claudegen/
```

---

## 주요 기능

### 플러그인 생성
```bash
claudegen generate                    # 플러그인 생성
claudegen generate --dry-run          # 설정만 확인
```

### 코드 분석
```bash
claudegen analyze                     # 코드 구조 분석
claudegen query "src/main.rs"         # 의존성 조회
claudegen validate                    # 출력 검증
```

### 관리
```bash
claudegen init                        # 프로젝트 초기화
claudegen status                      # 상태 확인
claudegen clean --all                 # 데이터 정리
claudegen config show                 # 설정 확인
```

---

## 설치

### Cargo
```bash
cargo install claudegen
```

### 소스 빌드
```bash
git clone https://github.com/junyeong-ai/claudegen && cd claudegen
cargo build --release
```

**요구사항**: Rust 1.92.0+

---

## Skill 형식 (SKILL.md)

```yaml
---
name: skill-name
description: "This skill should be used when..."
version: "1.0.0"
allowed-tools: "Read, Grep, Glob"
model: opus
context: fork
agent: agent-name
user-invocable: true
---

스킬 프롬프트 본문...
```

---

## 설정

`.claudegen/config.toml`:
```toml
[project]
name = "my-project"

[plugin]
name = ".claudegen"
version = "1.0.0"

[generation]
include_skills = true
include_agents = true
include_hooks = true
```

---

## 설정 우선순위

```
1. 내장 기본값
2. 글로벌 설정 (~/.claudegen/config.yaml)
3. 프로젝트 설정 (.claudegen/config.yaml)
4. 환경 변수 (CLAUDEGEN_*)
5. CLI 인수 (최우선)
```

---

## 문제 해결

```bash
# 데이터 초기화
claudegen clean --all && claudegen init

# 진행 상태 확인
claudegen status

# 디버그 모드
RUST_LOG=debug claudegen generate
```

---

## 지원

- [GitHub Issues](https://github.com/junyeong-ai/claudegen/issues)
- [개발자 가이드](CLAUDE.md)

---

<div align="center">

**[English](README.en.md)** | **한국어**

Made with Rust

</div>
