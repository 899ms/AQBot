[简体中文](./README.md) | [繁體中文](./README-ZH-TW.md) | [English](./README-EN.md) | [日本語](./README-JA.md) | **한국어** | [Français](./README-FR.md) | [Deutsch](./README-DE.md) | [Español](./README-ES.md) | [Русский](./README-RU.md) | [हिन्दी](./README-HI.md) | [العربية](./README-AR.md)

[![AQBot](https://socialify.git.ci/AQBot-Desktop/AQBot/image?description=1&font=JetBrains+Mono&forks=1&issues=1&logo=https%3A%2F%2Fgithub.com%2FAQBot-Desktop%2FAQBot%2Fblob%2Fmain%2Fsrc%2Fassets%2Fimage%2Flogo.png%3Fraw%3Dtrue&name=1&owner=1&pattern=Floating+Cogs&pulls=1&stargazers=1&theme=Auto)](https://github.com/AQBot-Desktop/AQBot)

AQBot은 여러 제공자의 채팅, ACP Agent, 지식 베이스, MCP 도구와 API 게이트웨이를 한곳에 모으고 앱 데이터와 사용자 파일을 직접 관리할 수 있게 하는 로컬 우선 데스크톱 AI 작업 공간입니다.

## 스크린샷

| 대화 차트 렌더링 | 서비스 제공업체 및 모델 |
|:---:|:---:|
| ![](.github/images/s1-0412.png) | ![](.github/images/s2-0412.png) |

| 지식 베이스 | 메모리 |
|:---:|:---:|
| ![](.github/images/s3-0412.png) | ![](.github/images/s4-0412.png) |

| Agent - 질문 | API 게이트웨이 원클릭 접속 |
|:---:|:---:|
| ![](.github/images/s5-0412.png) | ![](.github/images/s6-0412.png) |

| 대화 모델 선택 | 대화 탐색 |
|:---:|:---:|
| ![](.github/images/s7-0412.png) | ![](.github/images/s8-0412.png) |

| Agent - 권한 승인 | API 게이트웨이 개요 |
|:---:|:---:|
| ![](.github/images/s9-0412.png) | ![](.github/images/s10-0412.png) |

## 기능 목록

### 채팅 및 모델

- **멀티 제공업체 채팅** — OpenAI, Claude, Gemini, DeepSeek, Qwen 및 OpenAI 호환 엔드포인트를 Base URL, API Path, headers, proxy rules와 함께 연결합니다.
- **제공업체 온보딩** — aqbot:// provider links 및 CC Switch import로 사용자 확인 후 provider profiles를 AQBot으로 가져옵니다.
- **모델 관리** — remote model sync, groups, latency test, capabilities, context length, sampling defaults, reasoning profiles, per-model extra_body를 설정합니다.
- **대화 워크플로** — streaming replies, thinking blocks, message versions, branches, title-generation status, compression, multi-model comparison을 지원합니다.

### AI Agent

- **두 가지 Agent 사용 방식** — AQBot은 채팅에 내장된 Agent와 별도의 ACP Agent 워크벤치를 함께 제공합니다. 전자는 사용자가 설정한 제공자 API를 사용하고 후자는 ACP 호환 외부 Agent 프로세스에 연결하므로 모델과 작업 흐름에 맞게 선택할 수 있습니다.
- **채팅 Agent(제공자 API)** — 일반 대화를 Agent 모드로 전환하고 설정된 제공자와 모델 API를 그대로 사용하여 격리된 작업 디렉터리에서 파일 읽기/편집, 명령 실행과 코드 분석을 수행합니다.
- **채팅 Agent 제어** — 매번 확인, 편집 허용, 전체 접근 등의 권한 모드를 선택하고 도구 호출과 승인을 실시간으로 확인하며 실행별 token과 비용을 기록합니다.
- **ACP Agent 워크벤치** — [Agent Client Protocol (ACP)](https://agentclientprotocol.com/) 호환 코딩 Agent를 전용 화면에서 실행하고 응답, 추론 과정, 도구 호출을 스트리밍으로 확인합니다.
- **ACP Registry 및 사용자 지정 연동** — Registry에서 Codex, Claude Agent, Gemini CLI, Cline, OpenCode, Grok Build 등을 추가하거나 사용자 지정 명령, 인수, 환경 변수와 아이콘을 설정할 수 있으며 Agent 활성화와 순서 변경도 지원합니다.
- **ACP 프로젝트 및 바로 대화** — 스레드를 프로젝트별로 구성하고 중요한 세션을 고정하며 마지막 작업 상태를 자동 복원합니다. 프로젝트를 선택하지 않고 격리된 작업 디렉터리에서 바로 대화를 시작할 수도 있습니다.
- **완전한 ACP 세션 상호작용** — 각 Agent가 제공하는 모델, 모드와 옵션 전환은 물론 첨부 파일, 설문, 계획 검토, 진행 상황과 새로고침 후 상태 복원을 지원합니다. 도구 요청은 한 번만 허용하거나 현재 세션에서 ‘항상 허용’할 수 있습니다.

### 역할

- **로컬 역할 관리** — system prompt, avatar, tags, opening message, starter questions, temperature, Top P를 재사용 가능한 conversation template로 저장합니다.
- **원클릭 사용** — 기본 동작은 새 역할 대화 생성이며 dropdown에서 현재 대화에 적용할 수도 있습니다. 역할 대화는 이름, avatar, 파란색 역할 badge를 유지합니다.
- **온라인 마켓** — prompts.chat과 PlexPt 中文 source에서 역할을 검색하고 설치한 뒤 로컬 역할로 사용할 수 있습니다.

### Skills Management

- **Multi-source skill directories** — AQBot, Codex, Claude, Agents skill roots를 관리하며 `~/.aqbot/skills`, `~/.codex/skills`, `~/.claude/skills`, `~/.agents/skills`를 지원합니다.
- **My Skills** — source filter, enable/disable, detail view, copy name, open directory, uninstall을 지원합니다.
- **Skill groups and install targets** — group별 collapse, bulk enable/disable, group folder open, whole-group uninstall을 지원하고 `owner/repo` 또는 GitHub URL에서 선택한 target으로 설치합니다.
- **Marketplace** — skills.sh 및 GitHub source search, detail preview, GitHub jump, installed status를 제공합니다.

### 콘텐츠 렌더링

- **Markdown 및 수식** — Markdown, code highlighting, tables, task lists, LaTeX를 streaming conversation에서 렌더링합니다.
- **코드, 다이어그램, Artifact** — Monaco, Mermaid, D2, Artifact panel로 code, Markdown notes, reports, previews를 다룹니다.
- **HTML fragment rendering** — generated HTML fragments를 안전하게 preview하고 최근 streaming stability fixes를 반영합니다.

### 검색 및 지식

- **웹 검색** — Tavily, Exa, Zhipu WebSearch, Bocha 등과 cited sources, generated queries를 지원합니다.
- **로컬 지식베이스** — sqlite-vec로 private documents를 index하고 retrieval/rerank options 및 feedback을 확인합니다.
- **컨텍스트 관리** — files, search results, knowledge snippets, memories, tool output을 conversation context에 첨부합니다.

### 도구 및 확장

- **MCP protocol** — stdio, SSE, StreamableHTTP transport의 Model Context Protocol servers를 실행합니다.
- **Built-in tools** — @aqbot/fetch 및 file search 같은 built-in MCP tools를 별도 server 없이 사용합니다.
- **Tool loop limit** — MCP tool-call loop count를 설정하고 interrupted/stuck tool sessions에서 더 안정적으로 복구합니다.

### API 게이트웨이

- **Local gateway** — OpenAI Chat Completions, OpenAI Responses, Claude-native, Gemini-native endpoints를 desktop app에서 노출합니다.
- **Access and observability** — gateway keys, SSL/TLS certificates, request logs, usage analytics를 로컬에서 관리합니다.
- **Client templates** — Claude Code, Codex CLI, OpenCode, Gemini CLI, custom clients templates를 제공합니다.

### 데이터 가져오기 및 백업

- **Third-party imports** — ChatGPT official exports, Cherry Studio backups, Kelivo backups를 preview counts, warnings, duplicate handling과 함께 가져옵니다.
- **Provider and file migration** — Cherry Studio/Kelivo import에서 linked providers, API keys, file attachments를 선택적으로 migration합니다.
- **Backups** — local folders, WebDAV, S3-compatible storage로 backup/restore를 수행합니다.

### 데스크톱 및 보안

- **Local encryption** — app state는 ~/.aqbot/, user files는 ~/Documents/aqbot/에 저장되며 API keys는 AES-256 local master key로 보호됩니다.
- **Desktop integration** — tray, always-on-top, global shortcuts, auto-start, proxy settings, automatic update checks를 지원합니다.
- **11 interface languages** — Simplified/Traditional Chinese, English, Japanese, Korean, French, German, Spanish, Russian, Hindi, Arabic을 전환할 수 있습니다.

## 플랫폼 지원

| 플랫폼 | 아키텍처 |
|--------|---------|
| macOS | Apple Silicon (arm64), Intel (x86_64) |
| Windows 10/11 | x86_64, arm64 |
| Linux | x86_64 (AppImage/deb/rpm), arm64 (AppImage/deb/rpm) |

## 시작하기

[Releases](https://github.com/AQBot-Desktop/AQBot/releases) 페이지로 이동하여 플랫폼에 맞는 설치 프로그램을 다운로드하세요.

## 자주 묻는 질문

### macOS: "앱이 손상되었습니다" 또는 "개발자를 확인할 수 없습니다"

애플리케이션이 Apple에 의해 서명되지 않았기 때문에 macOS에서 다음 중 하나의 메시지가 표시될 수 있습니다:

- "AQBot"이 손상되어 열 수 없습니다
- Apple에서 악성 소프트웨어를 확인할 수 없어 "AQBot"을 열 수 없습니다

**해결 단계:**

**1. "모든 곳"에서 앱 허용**

```bash
sudo spctl --master-disable
```

그런 다음 **시스템 설정 → 개인 정보 보호 및 보안 → 보안**으로 이동하여 **모든 곳**을 선택하세요.

**2. 격리 속성 제거**

```bash
sudo xattr -dr com.apple.quarantine /Applications/AQBot.app
```

> 팁: 터미널에 `sudo xattr -dr com.apple.quarantine `을 입력한 후 앱 아이콘을 드래그할 수 있습니다.

**3. macOS Ventura 이상의 추가 단계**

위 단계를 완료한 후에도 첫 번째 실행이 차단될 수 있습니다. **시스템 설정 → 개인 정보 보호 및 보안**으로 이동하여 보안 섹션에서 **그래도 열기**를 클릭하세요. 이 작업은 한 번만 필요합니다.

## 커뮤니티
- [LinuxDO](https://linux.do)

## 라이선스

이 프로젝트는 [AGPL-3.0](LICENSE) 라이선스에 따라 배포됩니다.
