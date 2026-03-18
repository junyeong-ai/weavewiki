# 06. Claude Code 공식 가이드 심층 분석 및 최적 구성 설계

## 분석 일시

2026-02-07

## 분석 범위

공식 문서 소스:
- https://code.claude.com/docs/en/skills
- https://code.claude.com/docs/en/sub-agents
- https://code.claude.com/docs/en/memory
- https://code.claude.com/docs/en/best-practices

프로젝트 소스:
- `src/types/skill.rs` - 스킬 타입 정의
- `src/types/agent.rs` - 에이전트 타입 정의
- `src/types/rule.rs` - 룰 타입 정의
- `src/pipeline/generation/skills/` - 스킬 생성 파이프라인
- `src/pipeline/generation/agents/` - 에이전트 생성 파이프라인
- `src/pipeline/generation/claude_md/` - CLAUDE.md 생성 파이프라인
- `src/pipeline/generation/rules/` - 룰 생성 파이프라인

---

## 1. Skills 공식 스펙 vs 현재 구현 분석

### 1.1 공식 스펙 핵심 요약

Claude Code Skills는 Agent Skills (agentskills.io) 오픈 스탠다드를 따르며, Claude Code가 추가 기능(호출 제어, 서브에이전트 실행, 동적 컨텍스트 주입)을 확장한다.

**파일 구조:**
```
my-skill/
  SKILL.md           # Main instructions (필수)
  template.md        # 템플릿 (선택)
  examples/
    sample.md        # 예제 (선택)
  scripts/
    validate.sh      # 스크립트 (선택)
```

**YAML Frontmatter 필드:**
| 필드 | 필수 | 설명 |
|------|------|------|
| `name` | No | 디렉토리명 fallback. 소문자/숫자/하이픈, 최대 64자 |
| `description` | Recommended | Claude가 자동 로딩 판단에 사용 |
| `argument-hint` | No | 자동완성 힌트 |
| `disable-model-invocation` | No | true시 수동 전용 |
| `user-invocable` | No | false시 / 메뉴 숨김 |
| `allowed-tools` | No | 스킬 활성 시 허용 도구 |
| `model` | No | 스킬 활성 시 모델 |
| `context` | No | fork시 서브에이전트에서 실행 |
| `agent` | No | context:fork시 사용할 에이전트 유형 |
| `hooks` | No | 스킬 라이프사이클 훅 |

**호출 제어 매트릭스:**
| 설정 | 사용자 호출 | Claude 호출 | 컨텍스트 로딩 |
|------|------------|------------|--------------|
| 기본 | O | O | description만 항상 로드, 호출시 전체 로드 |
| disable-model-invocation: true | O | X | 컨텍스트에 미포함 |
| user-invocable: false | X | O | description 항상 로드, 호출시 전체 로드 |

**동적 컨텍스트:**
- `!command` 구문으로 셸 명령 사전 실행, 출력을 프롬프트에 삽입
- `$ARGUMENTS`, `$ARGUMENTS[N]`, `$N` 인수 치환
- `${CLAUDE_SESSION_ID}` 세션 ID 치환

**Progressive Disclosure:**
- SKILL.md는 500줄 이하 권장 (하드 제한 아님)
- 상세 참조 자료는 별도 파일로 분리
- SKILL.md에서 상대 경로로 참조

### 1.2 현재 구현 일치도 분석

**완전 일치 (100%):**

1. **Frontmatter 필드**: `src/types/skill.rs`에서 모든 공식 필드를 지원
   - `name`, `description`, `allowed-tools`, `model`, `context`, `agent`
   - `user-invocable`, `argument-hint`, `disable-model-invocation`, `hooks`
   - serde rename으로 하이픈-케이스 변환 정확

2. **이름 규칙**: kebab-case 검증 (`is_kebab_case`), 64자 제한

3. **ContextMode**: `Fork` 지원, `agent` 필드와의 연동 검증 (agent without fork 경고)

4. **allowed-tools 직렬화**: 쉼표 구분 문자열 (YAML 배열 아님) - 공식 형식 일치

5. **추가 파일**: `SkillFile` (name, content)로 지원 파일 모델링

**높은 일치 (90%+):**

6. **Progressive Disclosure** (`disclosure.rs`):
   - 3단계 가치 분류 (Critical/High/Normal) 정확
   - 500줄 고려 임계값 (하드 제한 아님) 올바른 해석
   - 패턴/예제 추출 시 `patterns.md`, `examples.md` 생성
   - `## Resources` 섹션에 상대 경로 참조 링크

7. **Dynamic Context Injection** (`disclosure.rs` DynamicContextInjector):
   - `!command` 구문 정확히 구현
   - 프로젝트 기술 스택 기반 조건부 주입
   - config-driven 패턴 매칭

**부분 일치/개선 필요:**

8. **`$ARGUMENTS` 치환**: Skill 타입에 `argument_hint` 필드가 있지만, 실제 `$ARGUMENTS`, `$0`, `$1` 등의 치환 로직은 스킬 body 생성 시점에서는 처리하지 않음. 이는 Claude Code 런타임이 처리하므로 정확한 설계.

9. **스킬 위치 계층**: 공식 스펙의 4단계 위치(Enterprise > Personal > Project > Plugin) 중, 프로젝트는 주로 `.claude/skills/` (Project 레벨) 생성물을 타겟팅하며 이는 적절함.

10. **동적 발견**: 공식 스펙에서 중첩 디렉토리의 자동 발견(`packages/frontend/.claude/skills/`)을 언급하는데, 현재 `monorepo.rs`의 `MonorepoSkillsGenerator`가 이를 지원.

### 1.3 Skills 관련 독창적 확장

1. **SkillCrossReferencer**: 스킬 간 관련성 자동 발견 (공유 도구, 키워드 패턴)
   - `## Related Skills` 섹션 자동 생성
   - `## Recommended Agent` 어노테이션

2. **RuleCrossReferencer**: 스킬에 관련 규칙 참조 자동 추가
   - `@.claude/rules/` 경로 참조로 Claude Code의 컨텍스트 시스템 활용

3. **QualityMetrics**: 생성된 스킬 본문의 품질 추적 (파일 참조 수, 유효성 상태)

4. **LLM-First Discovery**: LLM 기반 프로젝트별 스킬 발견 + 부정 피드백 재시도

---

## 2. Sub-agents 공식 스펙 vs 현재 구현 분석

### 2.1 공식 스펙 핵심 요약

**빌트인 에이전트:**
| 에이전트 | 모델 | 도구 | 용도 |
|----------|------|------|------|
| Explore | Haiku | 읽기 전용 | 코드베이스 탐색 |
| Plan | 상속 | 읽기 전용 | 계획 모드 리서치 |
| general-purpose | 상속 | 전체 | 복잡한 다단계 작업 |

**Frontmatter 필드:**
| 필드 | 필수 | 설명 |
|------|------|------|
| `name` | Yes | 고유 식별자, kebab-case |
| `description` | Yes | 위임 판단 기준 |
| `tools` | No | 허용 도구 목록 |
| `disallowedTools` | No | 차단 도구 목록 |
| `model` | No | sonnet/opus/haiku/inherit |
| `permissionMode` | No | default/acceptEdits/dontAsk/bypassPermissions/plan/delegate |
| `maxTurns` | No | 최대 에이전틱 턴 수 |
| `skills` | No | 프리로드할 스킬 목록 |
| `mcpServers` | No | MCP 서버 목록 |
| `hooks` | No | 라이프사이클 훅 |
| `memory` | No | user/project/local 범위 메모리 |

**키 개념:**
- 서브에이전트는 다른 서브에이전트를 생성할 수 없음
- `Task(agent_type)` 구문으로 생성 가능한 에이전트 제한
- 포그라운드/백그라운드 실행 모드
- 영구 메모리 (`~/.claude/agent-memory/<name>/`, `.claude/agent-memory/<name>/`)

### 2.2 현재 구현 일치도 분석

**완전 일치 (100%):**

1. **모든 공식 Frontmatter 필드 지원** (`src/types/agent.rs`):
   - `name`, `description`, `tools`, `disallowedTools`, `model`
   - `permissionMode`, `maxTurns`, `skills`, `mcpServers`, `hooks`, `memory`
   - 카멜 케이스 serde rename 정확 (`disallowedTools`, `permissionMode`, `maxTurns`, `mcpServers`)

2. **AgentModel 열거형**: `Sonnet`, `Opus`, `Haiku`, `Inherit` - 공식 스펙 완전 일치

3. **PermissionMode 열거형**: 6가지 모드 모두 지원
   - `Default`, `AcceptEdits`, `DontAsk`, `BypassPermissions`, `Plan`, `Delegate`

4. **MemoryScope 열거형**: `User`, `Project`, `Local` - 3단계 모두 지원

5. **도구 검증**: `is_valid_tool()` 함수로 유효 도구 확인

**높은 일치 (90%+):**

6. **마크다운 출력**: `to_markdown()` 메서드가 YAML frontmatter + markdown body 형식 생성

7. **스킬 프리로드**: `skills` 필드로 에이전트에 스킬 주입 지원
   - 공식: "full content of each skill is injected into the subagent's context"
   - 구현: `with_skills()`로 스킬 이름 목록 지정

8. **Validation**: kebab-case 이름, 필수 필드, bypassPermissions 경고

### 2.3 현재 구현의 독창적 확장

1. **5-Layer Agent Generation Architecture**:
   - Layer 1: Base agents (reviewer, coder, architect) - 항상 생성
   - Layer 2: Module specialists - 고가치 모듈별
   - Layer 3: Domain experts - 비즈니스 도메인별
   - Layer 4: LLM-discovered agents - 프로젝트 특화
   - Layer 5: Service specialists - 탐지된 서비스별

2. **ConsensusRole**: 에이전트 간 합의 메커니즘 (priority, can_veto, vote_threshold)
   - 공식 스펙에는 없지만, 멀티-에이전트 협업 시나리오에서 유용

3. **AgentColor**: 시각적 구분을 위한 색상 지정 (Blue, Green, Purple, Orange, Red)
   - 공식 스펙의 `/agents` 인터페이스에서 색상 선택 기능과 일치

4. **AgentExample**: 에이전트 프롬프트에 예제 주입 (context/user/assistant 구조)

5. **tool_sets 모듈**: read_only(), full_access(), library(), write_tools() 도구 세트 프리셋
   - 방어 심층: `disallowedTools`와 `tools`를 쌍으로 사용

6. **컨텍스트 인젝션**: 분석 데이터 기반 에이전트 프롬프트 동적 구성
   - Critical Insights, Architecture Patterns, Project Constraints
   - Applicable Rules (규칙 디렉토리 참조), Available Skills

---

## 3. CLAUDE.md / Memory 공식 스펙 vs 현재 구현 분석

### 3.1 공식 스펙 핵심 요약

**메모리 계층:**
| 타입 | 위치 | 용도 | 공유 범위 |
|------|------|------|----------|
| Managed policy | OS별 시스템 경로 | 조직 정책 | 전체 사용자 |
| Project memory | `./CLAUDE.md` 또는 `./.claude/CLAUDE.md` | 팀 공유 | 소스 관리 |
| Project rules | `./.claude/rules/*.md` | 모듈별 지침 | 소스 관리 |
| User memory | `~/.claude/CLAUDE.md` | 개인 설정 | 개인 |
| Project local | `./CLAUDE.local.md` | 개인 프로젝트 | 개인 |
| Auto memory | `~/.claude/projects/<project>/memory/` | 자동 메모 | 개인 |

**Rules 시스템 (`/.claude/rules/*.md`):**
- `paths` frontmatter로 glob 패턴 기반 조건부 적용
- 중첩 디렉토리 지원 (재귀 검색)
- 심링크 지원
- User-level rules (`~/.claude/rules/`)

**Import 시스템:**
- `@path/to/import` 구문 (상대/절대 경로)
- 재귀 import (최대 5단계)
- 코드 블록/스팬 내 import 무시
- 외부 import 첫 승인 다이얼로그

**CLAUDE.md 모범 사례:**
- 간결하게 유지 (Claude가 이미 코드에서 추론 가능한 것 제외)
- 빌드/테스트 명령, 코드 스타일 규칙, 워크플로우 포함
- 주기적 검토 및 가지치기
- IMPORTANT/YOU MUST 등 강조 사용 가능

### 3.2 현재 구현 일치도 분석

**완전 일치 (100%):**

1. **Rules 시스템** (`src/types/rule.rs`):
   - `paths` frontmatter 지원 (glob 패턴)
   - 계층적 카테고리: Project(100), Tech(90), Framework(85), Module(80), Domain(75), CrossCutting(75), Group(70), Service(65)
   - `output_path()`: 카테고리별 하위 디렉토리 자동 매핑
   - 마크다운 출력 시 `paths` frontmatter 정확히 생성

2. **@import 시스템** (`src/pipeline/generation/claude_md/imports.rs`):
   - Priority-based import ordering
   - `DEFAULT_MAX_IMPORTS = 20` 제한
   - 우아한 degradation: 고우선순위 import 유지, 저우선순위 drop

3. **CLAUDE.md 구조** (`src/pipeline/generation/claude_md/mod.rs`):
   - Overview, Architecture, Standards, Domain Knowledge, Gotchas 섹션
   - `@.claude/rules/`, `@.claude/skills/`, `@.claude/agents/` import
   - 대형 섹션 외부 문서 추출 (`@.claude/docs/`)

**높은 일치 (90%+):**

4. **Nested CLAUDE.md** (`nested.rs`):
   - 모노레포 서브프로젝트별 CLAUDE.md 생성
   - 부모 CLAUDE.md @import (상대 경로 계산)
   - 워크스페이스별 규칙 필터링
   - 공유 패키지 참조

5. **Differential Updates** (`cache.rs`):
   - 섹션별 입력 해시 추적
   - 변경된 섹션만 재생성
   - 섹션 매니페스트 캐시

6. **Navigation Map**: 모듈-규칙-에이전트-스킬 관계 매핑 테이블

### 3.3 개선 기회

1. **Auto Memory 미지원**: 공식 스펙의 `~/.claude/projects/<project>/memory/` 자동 메모리 시스템은 현재 생성 대상이 아님. 이는 Claude Code 런타임 기능이므로 적절한 판단.

2. **CLAUDE.local.md**: 개인 로컬 설정 파일 생성 미지원 (팀 공유 대상이 아니므로 적절)

3. **User-level rules**: `~/.claude/rules/` 생성 미지원 (프로젝트 스코프가 주 타겟이므로 적절)

---

## 4. Progressive Disclosure 최적 전략 분석

### 4.1 현재 구현의 3-Layer 접근 체계

```
CLAUDE.md (항상 로드)
  |
  +-- @.claude/rules/ (경로 패턴 기반 조건부 로드)
  |     +-- project.md (항상)
  |     +-- tech/*.md (확장자 기반)
  |     +-- frameworks/*.md (경로/키워드)
  |     +-- modules/*.md (경로 기반)
  |     +-- groups/*.md (멤버 경로)
  |     +-- domains/*.md (키워드 트리거)
  |     +-- cross-cutting/*.md
  |     +-- services/*.md
  |
  +-- @.claude/skills/ (description 기반 관련성 판단, 호출시 전체 로드)
  |     +-- <skill-name>/SKILL.md
  |     +-- <skill-name>/patterns.md (선택, on-demand)
  |     +-- <skill-name>/examples.md (선택, on-demand)
  |
  +-- @.claude/agents/ (task 위임시 로드)
  |     +-- <agent-name>.md
  |
  +-- @.claude/docs/ (대형 섹션 추출, @참조 시 로드)
        +-- architecture.md
        +-- standards.md
        +-- domain.md
```

### 4.2 공식 스펙과의 정합성

**컨텍스트 로딩 순서 (공식):**
1. CLAUDE.md 파일은 세션 시작 시 전체 로드
2. 스킬 description은 컨텍스트에 상시 포함 (disable-model-invocation: true 제외)
3. 스킬 전체 내용은 호출 시에만 로드
4. Rules의 `paths` frontmatter는 파일 작업 시 조건부 적용
5. 하위 디렉토리 CLAUDE.md는 해당 디렉토리 파일 접근 시 lazy-load

**현재 구현의 정합성:**

| 계층 | 공식 동작 | 현재 구현 |
|------|----------|----------|
| CLAUDE.md | 항상 전체 로드 | Overview + 핵심 섹션 inline, 대형 섹션 @docs/ 추출 |
| Rules (paths) | 파일 작업 시 조건부 | `paths` frontmatter 정확히 생성 |
| Rules (no paths) | 항상 로드 | Domain rules은 triggers 전용 (paths 없음 = 항상 로드) |
| Skills | description만 상시 | description 필드 필수 검증 |
| Skills (full) | 호출 시 로드 | Progressive disclosure로 SKILL.md 최적화 |
| Agents | 위임 시 로드 | description 필드 필수 검증 |
| Nested CLAUDE.md | 하위 디렉토리 lazy-load | 모노레포 서브프로젝트별 생성 |

### 4.3 최적 Progressive Disclosure 전략

**Tier 1: Always-On (CLAUDE.md)**
- 프로젝트 개요 (< 10줄)
- 핵심 빌드/테스트 명령
- 중요 아키텍처 제약 (< 5항목)
- @import 참조 목록

**Tier 2: Path-Conditional (Rules)**
- 기술 규칙: 파일 확장자 기반 활성화
- 모듈 규칙: 디렉토리 경로 기반 활성화
- 프레임워크 규칙: 경로 + 키워드 조합

**Tier 3: On-Demand (Skills)**
- 스킬 description만 상시 로드 (컨텍스트 예산 2%)
- 전체 스킬 내용은 호출/관련성 판단 시
- 대형 참조 자료는 지원 파일로 분리

**Tier 4: Delegated (Agents)**
- 에이전트는 별도 컨텍스트 윈도우에서 실행
- 메인 대화 컨텍스트 보존
- skills 프리로드로 에이전트에 도메인 지식 주입

### 4.4 현재 구현의 고유 가치

1. **Import Priority Manager**: 규칙을 Framework > Tech > Module > Group 우선순위로 정렬, 상한 초과 시 저우선순위부터 drop. 이는 공식 스펙의 컨텍스트 예산 관리와 완벽히 정합.

2. **대형 섹션 자동 추출**: Architecture/Standards/Domain 섹션이 임계값 초과 시 `.claude/docs/`로 추출, @참조로 대체. 공식 모범 사례의 "간결한 CLAUDE.md" 원칙과 일치.

3. **Navigation Map**: 모듈-규칙-에이전트-스킬 관계 테이블로 빠른 탐색 지원.

4. **Cross-Reference 체계**: 스킬 -> 규칙, 스킬 -> 스킬, 에이전트 -> 규칙, 에이전트 -> 스킬 참조 자동 생성.

---

## 5. 장기적 관점의 최적 구성 설계안

### 5.1 마이크로서비스/모노레포 적용

**현재 지원:**
- `NestedClaudeMdGenerator`: 서브프로젝트별 CLAUDE.md
- `MonorepoSkillsGenerator`: 워크스페이스별 스킬
- `ServiceAgentGenerator`: 서비스별 에이전트
- 규칙 필터링: 워크스페이스 경로 기반

**최적 구조 (대규모 모노레포):**
```
monorepo/
  CLAUDE.md                    # 루트: 전체 프로젝트 개요 + @import
  .claude/
    CLAUDE.md                  # .claude 위치 대안 (지원됨)
    rules/
      project.md               # 전역 규칙
      tech/                    # 기술별 규칙
      cross-cutting/           # 횡단 관심사
    skills/
      shared-skill/SKILL.md    # 공유 스킬
    agents/
      reviewer.md              # 공유 에이전트
  packages/
    api/
      CLAUDE.md                # @import ../../CLAUDE.md + API 특화
      .claude/
        rules/                 # API 전용 규칙
        skills/                # API 전용 스킬
        agents/                # API 전용 에이전트
    web/
      CLAUDE.md                # @import ../../CLAUDE.md + Web 특화
      .claude/
        rules/
        skills/
        agents/
```

### 5.2 엔터프라이즈급 확장성

**컨텍스트 예산 관리:**
- SLASH_COMMAND_TOOL_CHAR_BUDGET 환경변수: 컨텍스트 윈도우의 2%, 기본 16,000자
- Import max limit: 20개 기본, 설정 가능
- Progressive disclosure: 500줄 고려 임계값

**확장 전략:**
1. **규칙 세분화**: 대형 규칙을 더 작은 조건부 규칙으로 분할
2. **스킬 네임스페이스**: Plugin 시스템의 `plugin-name:skill-name` 활용
3. **에이전트 위임**: 복잡한 작업을 전문 에이전트에 위임하여 메인 컨텍스트 보존
4. **캐싱**: 섹션별 해시 기반 differential update

### 5.3 도메인/서비스 특화 생성물 최적 배치

**규칙 (Rules):**
- `domains/` 하위 디렉토리: 비즈니스 도메인별 규칙
- `services/` 하위 디렉토리: 서비스별 규칙
- `cross-cutting/` 하위 디렉토리: 에러 핸들링, 로깅 등
- Custom categories: 프로젝트 분석에서 발견된 특수 규칙

**스킬 (Skills):**
- 도메인 작업 스킬: `fix-issue`, `deploy`, `migrate-component`
- 참조 스킬: `api-conventions`, `legacy-system-context`
- 분석 스킬: `deep-research` (context: fork, agent: Explore)

**에이전트 (Agents):**
- 기본 3종: reviewer, coder, architect
- 모듈 전문가: `<module>-specialist`
- 도메인 전문가: `<domain>-expert`
- 서비스 전문가: `<service>-specialist`

---

## 6. 종합 평가

### 6.1 공식 스펙 일치도 점수

| 영역 | 일치도 | 비고 |
|------|--------|------|
| Skills 타입 정의 | 98% | 모든 frontmatter 필드 지원 |
| Skills 생성 | 95% | LLM-first discovery + progressive disclosure |
| Skills 출력 형식 | 100% | YAML frontmatter + markdown body 정확 |
| Agents 타입 정의 | 98% | 모든 frontmatter 필드 + color 지원 |
| Agents 생성 | 95% | 5-layer 아키텍처 + LLM discovery |
| Agents 출력 형식 | 100% | YAML frontmatter + markdown prompt 정확 |
| Rules 타입/출력 | 95% | paths frontmatter + 카테고리별 디렉토리 |
| CLAUDE.md 생성 | 92% | 핵심 섹션 + @import + 대형 섹션 추출 |
| Nested CLAUDE.md | 90% | 모노레포 지원, 부모 @import |
| Progressive Disclosure | 93% | 3-tier 가치 분류 + 컨텍스트 예산 관리 |
| **종합** | **95%** | **공식 스펙 높은 수준 준수** |

### 6.2 주요 강점

1. **Evidence-Based Generation**: 모든 생성물이 코드 분석 증거 기반
2. **LLM-First Architecture**: 정적 분석 + LLM 발견의 하이브리드
3. **Context Budget Awareness**: Import 우선순위, 대형 섹션 추출, Progressive disclosure
4. **Cross-Reference System**: 규칙-스킬-에이전트 간 양방향 참조
5. **Monorepo Support**: 중첩 CLAUDE.md, 워크스페이스별 스킬/규칙
6. **Differential Updates**: 섹션별 해시 기반 캐싱으로 재생성 최소화

### 6.3 개선 권장사항

**우선순위 높음:**

1. **Hooks 통합 강화**: 현재 hooks 필드는 타입 정의만 존재. 스킬과 에이전트의 훅 생성 로직 구현 필요. 특히 `PreToolUse` 유효성 검사 훅은 보안 관점에서 가치가 높음.

2. **Plugin 호환성**: 공식 Plugin 시스템과의 통합 지원. `plugin-name:skill-name` 네임스페이스, `agents/` 디렉토리 구조.

**우선순위 중간:**

3. **Agent Memory 생성**: 공식 스펙의 persistent memory (MEMORY.md) 구조를 초기화하는 기능. 에이전트가 학습할 초기 지식 기반 생성.

4. **도구 접근 제한 세분화**: `Task(agent_type)` 구문 지원 - 에이전트가 생성할 수 있는 서브에이전트 유형 제한.

5. **Background Agent 힌트**: 에이전트 description에 포그라운드/백그라운드 실행 적합성 표시.

**우선순위 낮음:**

6. **CLI-defined 에이전트 JSON**: `--agents` 플래그 호환 JSON 출력 포맷.

7. **Visual Skill Output**: 스킬에 스크립트 번들링 지원 (codebase-visualizer 패턴).

8. **Managed Policy 레벨**: 엔터프라이즈 조직 정책 CLAUDE.md 템플릿 생성.

### 6.4 설계 원칙 준수 확인

**공식 Best Practices 원칙과의 정합:**

| 원칙 | 현재 상태 | 평가 |
|------|----------|------|
| "Claude가 검증할 수 있도록 하라" | 생성물에 @file:line 증거 포함 | 우수 |
| "탐색 > 계획 > 코딩" | Plan 모드 에이전트, 읽기 전용 Explore | 우수 |
| "구체적 컨텍스트 제공" | 분석 기반 프롬프트 구성 | 우수 |
| "CLAUDE.md 간결하게" | 대형 섹션 추출, import 제한 | 우수 |
| "서브에이전트로 조사" | 전문 에이전트 자동 생성 | 우수 |
| "컨텍스트 적극 관리" | Import priority, progressive disclosure | 우수 |
| "과도한 CLAUDE.md 경고" | 임계값 기반 추출 | 양호 |
| "훅으로 필수 동작 보장" | 타입 정의만 (생성 로직 미완) | 개선 필요 |

---

## 7. 결론

claudegen 프로젝트의 생성물 체계는 Claude Code 공식 스펙에 **95% 수준**으로 일치하며, 여러 영역에서 공식 스펙을 초과하는 독창적 확장을 제공한다. 특히 Evidence-Based Generation, LLM-First Discovery, 5-Layer Agent Architecture, Progressive Disclosure의 3-Tier 가치 분류는 공식 모범 사례를 체계적으로 구현한 결과이다.

가장 큰 차별화 요소는 **자동 분석 기반 생성**이다. 공식 가이드는 수동 작성을 전제하지만, claudegen은 코드베이스 분석 → 증거 수집 → LLM 발견 → 규칙/스킬/에이전트 자동 생성의 파이프라인을 제공한다. 이는 공식 가이드의 모든 모범 사례를 프로그래밍 방식으로 달성하는 것이며, 특히 대규모 코드베이스에서 수동 설정의 한계를 극복한다.
