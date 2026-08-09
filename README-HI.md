[简体中文](./README.md) | [繁體中文](./README-ZH-TW.md) | [English](./README-EN.md) | [日本語](./README-JA.md) | [한국어](./README-KO.md) | [Français](./README-FR.md) | [Deutsch](./README-DE.md) | [Español](./README-ES.md) | [Русский](./README-RU.md) | **हिन्दी** | [العربية](./README-AR.md)

[![AQBot](https://socialify.git.ci/AQBot-Desktop/AQBot/image?description=1&font=JetBrains+Mono&forks=1&issues=1&logo=https%3A%2F%2Fgithub.com%2FAQBot-Desktop%2FAQBot%2Fblob%2Fmain%2Fsrc%2Fassets%2Fimage%2Flogo.png%3Fraw%3Dtrue&name=1&owner=1&pattern=Floating+Cogs&pulls=1&stargazers=1&theme=Auto)](https://github.com/AQBot-Desktop/AQBot)

AQBot एक local-first desktop AI workspace है जो multi-provider chat, ACP agents, knowledge bases, MCP tools और API gateway को एक साथ लाता है, जबकि app data और user files आपके नियंत्रण में रहते हैं।

## स्क्रीनशॉट

| चैट चार्ट रेंडरिंग | प्रदाता और मॉडल |
|:---:|:---:|
| ![](.github/images/s1-0412.png) | ![](.github/images/s2-0412.png) |

| ज्ञान आधार | स्मृति |
|:---:|:---:|
| ![](.github/images/s3-0412.png) | ![](.github/images/s4-0412.png) |

| Agent - पूछताछ | API गेटवे वन-क्लिक एक्सेस |
|:---:|:---:|
| ![](.github/images/s5-0412.png) | ![](.github/images/s6-0412.png) |

| चैट मॉडल चयन | चैट नेविगेशन |
|:---:|:---:|
| ![](.github/images/s7-0412.png) | ![](.github/images/s8-0412.png) |

| Agent - अनुमति अनुमोदन | API गेटवे अवलोकन |
|:---:|:---:|
| ![](.github/images/s9-0412.png) | ![](.github/images/s10-0412.png) |

## विशेषताएँ

### चैट और मॉडल

- **Multi-provider chat** — OpenAI, Claude, Gemini, DeepSeek, Qwen और OpenAI-compatible endpoints को Base URL, API Path, headers और proxy rules के साथ जोड़ें।
- **Provider onboarding** — aqbot:// provider links और CC Switch import से user confirmation के बाद provider profiles AQBot में लाएं।
- **Model management** — Remote model lists sync करें, groups व्यवस्थित करें, latency test करें और capabilities, context length, sampling defaults, reasoning profiles तथा per-model extra_body सेट करें।
- **Conversation workflows** — Streaming replies, thinking blocks, message versions, branches, title-generation status, long chat compression और multi-model comparison।

### AI Agent

- **Agents इस्तेमाल करने के दो तरीके** — AQBot chat में built-in agent और अलग ACP Agent workbench दोनों देता है। पहला आपके configure किए गए provider APIs का उपयोग करता है, जबकि दूसरा ACP-compatible external agent processes से जुड़ता है, ताकि model और workflow के अनुसार सही तरीका चुना जा सके।
- **Chat agent (provider API)** — सामान्य conversation को Agent mode में बदलें और configure किए गए provider तथा model API से isolated working directory में files पढ़ें या edit करें, commands चलाएँ और code analyze करें।
- **Chat agent controls** — हर बार पूछने, edits स्वीकार करने या full access जैसे permission modes चुनें, tool calls और approvals real time में review करें तथा हर run के token और cost track करें।
- **ACP Agent workbench** — [Agent Client Protocol (ACP)](https://agentclientprotocol.com/) के ज़रिए compatible coding agents को dedicated workbench में चलाएँ और responses, reasoning तथा tool calls को stream होते देखें।
- **ACP Registry और custom integrations** — Registry से Codex, Claude Agent, Gemini CLI, Cline, OpenCode और Grok Build जैसे agents जोड़ें, या custom command, arguments, environment variables और icon configure करें; agents को enable और reorder भी किया जा सकता है।
- **ACP projects और direct chats** — Threads को project के अनुसार व्यवस्थित करें, महत्वपूर्ण sessions pin करें और पिछला workspace अपने आप restore करें, या project चुने बिना isolated working directory में direct chat शुरू करें।
- **पूरी ACP session interaction** — हर agent द्वारा उपलब्ध models, modes और options बदलें तथा attachments, questionnaires, plan review, plan progress और reload के बाद persisted state के साथ काम करें; tool requests को एक बार या मौजूदा session में “Always Allow” किया जा सकता है।

### Roles

- **Local role management** — System prompts, avatars, tags, opening messages, starter questions, temperature और Top P को reusable conversation templates की तरह save करें।
- **One-click use** — Default में नया role conversation बनता है, dropdown से current conversation पर apply भी कर सकते हैं; role chats name, avatar और blue Roles badge रखते हैं।
- **Online marketplace** — prompts.chat और PlexPt 中文 sources से roles search/install करें और local roles की तरह use करें।

### Skills Management

- **Multi-source skill directories** — AQBot, Codex, Claude और Agents skill roots manage करें, जिनमें `~/.aqbot/skills`, `~/.codex/skills`, `~/.claude/skills` और `~/.agents/skills` शामिल हैं।
- **My Skills** — Source filter, enable/disable, detail view, copy name, open directory और uninstall support।
- **Skill groups और install targets** — group के आधार पर collapse, bulk enable/disable, group folder open, whole-group uninstall करें और `owner/repo` या GitHub URL से चुने हुए target में install करें।
- **Marketplace** — skills.sh और GitHub sources search करें, details preview करें, GitHub खोलें और installed status देखें।

### सामग्री रेंडरिंग

- **Markdown और math** — Streaming conversations में Markdown, code highlighting, tables, task lists और LaTeX formulas render करें।
- **Code, diagrams और Artifact** — Monaco code blocks, Mermaid, D2 और Artifact panel से code, Markdown notes, reports और previews देखें।
- **HTML fragments** — Generated HTML fragments सुरक्षित preview करें, हाल के streaming stability fixes के साथ।

### खोज और ज्ञान

- **Web search** — Tavily, Exa, Zhipu WebSearch, Bocha आदि cited sources और generated search queries के साथ।
- **Local knowledge base** — sqlite-vec से private documents index करें, retrieval/rerank options tune करें और retrieval feedback देखें।
- **Context management** — Files, search results, knowledge snippets, memories और tool output को conversation context में जोड़ें।

### टूल और एक्सटेंशन

- **MCP protocol** — stdio, SSE या StreamableHTTP transport वाले Model Context Protocol servers चलाएं।
- **Built-in tools** — @aqbot/fetch और file search जैसे built-in MCP tools बिना अलग server के उपयोग करें।
- **Tool loop limit** — MCP tool-call loop count सेट करें और interrupted/stuck tool sessions से बेहतर recover करें।

### API gateway

- **Local gateway** — Desktop app से OpenAI Chat Completions, OpenAI Responses, Claude-native और Gemini-native endpoints expose करें।
- **Access और observability** — Gateway keys, SSL/TLS certificates, request logs और usage analytics local रूप से manage करें।
- **Client templates** — Claude Code, Codex CLI, OpenCode, Gemini CLI और custom clients के ready templates।

### Data import और backup

- **Third-party imports** — ChatGPT official exports, Cherry Studio backups और Kelivo backups preview counts, warnings और duplicate handling के साथ import करें।
- **Provider और file migration** — Cherry Studio/Kelivo import linked providers, API keys और file attachments optionally migrate कर सकता है।
- **Backups** — Local folders, WebDAV या S3-compatible storage से backup और restore करें।

### Desktop और security

- **Local encryption** — App state ~/.aqbot/ में, user files ~/Documents/aqbot/ में, API keys AES-256 और local master key से protected।
- **Desktop integration** — Tray, always-on-top, global shortcuts, auto-start, proxy settings और automatic update checks।
- **11 interface languages** — Simplified Chinese, Traditional Chinese, English, Japanese, Korean, French, German, Spanish, Russian, Hindi और Arabic में switch करें।

## प्लेटफॉर्म समर्थन

| प्लेटफॉर्म | आर्किटेक्चर |
|-----------|------------|
| macOS | Apple Silicon (arm64), Intel (x86_64) |
| Windows 10/11 | x86_64, arm64 |
| Linux | x86_64 (AppImage/deb/rpm), arm64 (AppImage/deb/rpm) |

## शुरू करना

[Releases](https://github.com/AQBot-Desktop/AQBot/releases) पेज पर जाएँ और अपने प्लेटफॉर्म के लिए इंस्टॉलर डाउनलोड करें।

## अक्सर पूछे जाने वाले प्रश्न

### macOS: "ऐप क्षतिग्रस्त है" या "डेवलपर को सत्यापित नहीं किया जा सकता"

चूँकि एप्लिकेशन Apple द्वारा साइन नहीं किया गया है, macOS निम्नलिखित में से एक संकेत दिखा सकता है:

- "AQBot" क्षतिग्रस्त है और इसे नहीं खोला जा सकता
- "AQBot" को नहीं खोला जा सकता क्योंकि Apple इसे दुर्भावनापूर्ण सॉफ़्टवेयर के लिए जाँच नहीं कर सकता

**समाधान के चरण:**

**1. "कहीं से भी" ऐप्स की अनुमति दें**

```bash
sudo spctl --master-disable
```

फिर **सिस्टम सेटिंग्स → गोपनीयता और सुरक्षा → सुरक्षा** पर जाएँ और **कहीं से भी** चुनें।

**2. क्वारंटाइन विशेषता हटाएँ**

```bash
sudo xattr -dr com.apple.quarantine /Applications/AQBot.app
```

> टिप: आप `sudo xattr -dr com.apple.quarantine ` टाइप करने के बाद टर्मिनल पर ऐप आइकन खींच सकते हैं।

**3. macOS Ventura और बाद के संस्करणों के लिए अतिरिक्त चरण**

उपरोक्त चरण पूरे करने के बाद, पहला लॉन्च अभी भी ब्लॉक हो सकता है। **सिस्टम सेटिंग्स → गोपनीयता और सुरक्षा** पर जाएँ, फिर सुरक्षा अनुभाग में **फिर भी खोलें** पर क्लिक करें। यह केवल एक बार किया जाना चाहिए।

## समुदाय
- [LinuxDO](https://linux.do)

## लाइसेंस

यह प्रोजेक्ट [AGPL-3.0](LICENSE) लाइसेंस के तहत लाइसेंस प्राप्त है।
