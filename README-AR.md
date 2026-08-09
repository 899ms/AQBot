[简体中文](./README.md) | [繁體中文](./README-ZH-TW.md) | [English](./README-EN.md) | [日本語](./README-JA.md) | [한국어](./README-KO.md) | [Français](./README-FR.md) | [Deutsch](./README-DE.md) | [Español](./README-ES.md) | [Русский](./README-RU.md) | [हिन्दी](./README-HI.md) | **العربية**

[![AQBot](https://socialify.git.ci/AQBot-Desktop/AQBot/image?description=1&font=JetBrains+Mono&forks=1&issues=1&logo=https%3A%2F%2Fgithub.com%2FAQBot-Desktop%2FAQBot%2Fblob%2Fmain%2Fsrc%2Fassets%2Fimage%2Flogo.png%3Fraw%3Dtrue&name=1&owner=1&pattern=Floating+Cogs&pulls=1&stargazers=1&theme=Auto)](https://github.com/AQBot-Desktop/AQBot)

AQBot هو مساحة عمل مكتبية للذكاء الاصطناعي تعتمد على التخزين المحلي، وتجمع المحادثة عبر عدة مزودين ووكلاء ACP وقواعد المعرفة وأدوات MCP وبوابة API، مع إبقاء بيانات التطبيق وملفات المستخدم تحت سيطرتك.

## لقطات الشاشة

| عرض مخططات المحادثة | مزودو الخدمة والنماذج |
|:---:|:---:|
| ![](.github/images/s1-0412.png) | ![](.github/images/s2-0412.png) |

| قاعدة المعرفة | الذاكرة |
|:---:|:---:|
| ![](.github/images/s3-0412.png) | ![](.github/images/s4-0412.png) |

| Agent - استفسار | بوابة API بنقرة واحدة |
|:---:|:---:|
| ![](.github/images/s5-0412.png) | ![](.github/images/s6-0412.png) |

| اختيار نموذج المحادثة | تنقل المحادثات |
|:---:|:---:|
| ![](.github/images/s7-0412.png) | ![](.github/images/s8-0412.png) |

| Agent - الموافقة على الصلاحيات | نظرة عامة على بوابة API |
|:---:|:---:|
| ![](.github/images/s9-0412.png) | ![](.github/images/s10-0412.png) |

## الميزات

### الدردشة والنماذج

- **Multi-provider chat** — اربط OpenAI وClaude وGemini وDeepSeek وQwen وأي OpenAI-compatible endpoint مع Base URL وAPI Path وheaders وproxy rules.
- **تهيئة المزوّدين** — استخدم aqbot:// provider links وCC Switch import لجلب provider profiles إلى AQBot بعد تأكيد المستخدم.
- **إدارة النماذج** — زامن remote model lists، ونظّم groups، واختبر latency، واضبط capabilities وcontext length وsampling defaults وreasoning profiles وper-model extra_body.
- **مسارات المحادثة** — Streaming replies وthinking blocks وmessage versions وbranches وtitle-generation status وlong chat compression وmulti-model comparison.

### AI Agent

- **طريقتان لاستخدام الوكلاء** — يوفر AQBot وكيلاً مدمجًا في المحادثة ومساحة عمل مستقلة لوكلاء ACP. يستخدم الأول واجهات API لمزودي الخدمة التي يضبطها المستخدم، بينما يتصل الثاني بعمليات وكلاء خارجية متوافقة مع ACP، لتختار الأنسب للنموذج وسير العمل.
- **وكيل المحادثة (API مزود الخدمة)** — حوّل أي محادثة عادية إلى وضع Agent واستخدم API المزود والنموذج المضبوطين مباشرةً لقراءة الملفات أو تعديلها وتشغيل الأوامر وتحليل الشيفرة داخل مجلد عمل معزول.
- **التحكم في وكيل المحادثة** — اختر أوضاع الصلاحيات مثل السؤال في كل مرة أو قبول التعديلات أو الوصول الكامل، وراجع استدعاءات الأدوات والموافقات لحظيًا، وتابع الرموز والتكلفة لكل تشغيل.
- **مساحة عمل وكلاء ACP** — شغّل وكلاء البرمجة المتوافقين عبر [Agent Client Protocol (ACP)](https://agentclientprotocol.com/) في مساحة عمل مستقلة، مع بث الردود والاستدلال واستدعاءات الأدوات.
- **ACP Registry والتكاملات المخصصة** — أضف من Registry وكلاء مثل Codex وClaude Agent وGemini CLI وCline وOpenCode وGrok Build، أو اضبط أمرًا ووسائط ومتغيرات بيئة وأيقونة مخصصة؛ ويمكنك تفعيل الوكلاء وإعادة ترتيبهم بحرية.
- **مشاريع ACP والمحادثات المباشرة** — نظّم المواضيع حسب المشروع، وثبّت الجلسات المهمة واستعد مساحة العمل الأخيرة تلقائيًا، أو ابدأ محادثة بلا مشروع داخل مجلد عمل معزول.
- **تفاعل متكامل مع جلسات ACP** — بدّل النماذج والأوضاع والخيارات التي يوفرها كل وكيل، واستخدم المرفقات والاستبيانات ومراجعة الخطط وتتبع تقدمها واستعادة الحالة بعد إعادة التحميل؛ ويمكن السماح بطلبات الأدوات مرة واحدة أو دائمًا في الجلسة الحالية.

### Roles

- **Local role management** — احفظ system prompts وavatars وtags وopening messages وstarter questions وtemperature وTop P كقوالب conversation قابلة لإعادة الاستخدام.
- **One-click use** — السلوك الافتراضي ينشئ role conversation جديداً، ويمكن تطبيق role على المحادثة الحالية من dropdown؛ role chats تحتفظ بالاسم وavatar وblue Roles badge.
- **Online marketplace** — ابحث وثبّت roles من prompts.chat وPlexPt 中文 ثم استخدمها محلياً.

### Skills Management

- **Multi-source skill directories** — أدر AQBot وCodex وClaude وAgents skill roots، بما فيها `~/.aqbot/skills` و`~/.codex/skills` و`~/.claude/skills` و`~/.agents/skills`.
- **My Skills** — يدعم source filter وenable/disable وdetail view وcopy name وopen directory وuninstall.
- **Skill groups and install targets** — اطوِ حسب group، ونفّذ bulk enable/disable، وافتح group folder، واحذف whole group، وثبّت من `owner/repo` أو GitHub URL إلى target تختاره.
- **Marketplace** — ابحث في skills.sh وGitHub، وعاين details، وافتح GitHub، واعرض installed status.

### عرض المحتوى

- **Markdown والمعادلات** — اعرض Markdown وcode highlighting وtables وtask lists وLaTeX formulas داخل streaming conversations.
- **الكود والمخططات وArtifact** — استخدم Monaco code blocks وMermaid وD2 وArtifact panel للكود والملاحظات والتقارير والمعاينات.
- **HTML fragments** — عاين generated HTML fragments بأمان مع أحدث streaming stability fixes.

### البحث والمعرفة

- **Web search** — يدعم Tavily وExa وZhipu WebSearch وBocha مع cited sources وgenerated search queries.
- **Local knowledge base** — افهرس private documents باستخدام sqlite-vec واضبط retrieval/rerank options وراجع retrieval feedback.
- **Context management** — أضف files وsearch results وknowledge snippets وmemories وtool output إلى conversation context.

### الأدوات والامتدادات

- **MCP protocol** — شغّل Model Context Protocol servers عبر stdio أو SSE أو StreamableHTTP.
- **Built-in tools** — استخدم @aqbot/fetch وfile search بدون تثبيت server منفصل.
- **Tool loop limit** — اضبط MCP tool-call loop count واستعد بشكل أفضل من interrupted أو stuck tool sessions.

### API gateway

- **Local gateway** — اعرض OpenAI Chat Completions وOpenAI Responses وClaude-native وGemini-native endpoints من desktop app.
- **الوصول والمراقبة** — أدر gateway keys وSSL/TLS certificates وrequest logs وusage analytics محلياً.
- **Client templates** — Templates جاهزة لـ Claude Code وCodex CLI وOpenCode وGemini CLI وcustom clients.

### استيراد البيانات والنسخ الاحتياطي

- **Third-party imports** — استورد ChatGPT official exports وCherry Studio backups وKelivo backups مع preview counts وwarnings وduplicate handling.
- **Provider and file migration** — يمكن لـ Cherry Studio/Kelivo import نقل linked providers وAPI keys وfile attachments اختيارياً.
- **Backups** — انسخ واستعد البيانات عبر local folders أو WebDAV أو S3-compatible storage.

### سطح المكتب والأمان

- **Local encryption** — يُحفظ app state في ~/.aqbot/ وuser files في ~/Documents/aqbot/، وتُحمى API keys عبر AES-256 وlocal master key.
- **Desktop integration** — Tray وalways-on-top وglobal shortcuts وauto-start وproxy settings وautomatic update checks.
- **11 interface languages** — بدّل بين Simplified Chinese وTraditional Chinese وEnglish وJapanese وKorean وFrench وGerman وSpanish وRussian وHindi وArabic.

## دعم المنصات

| المنصة | البنية |
|--------|--------|
| macOS | Apple Silicon (arm64), Intel (x86_64) |
| Windows 10/11 | x86_64, arm64 |
| Linux | x86_64 (AppImage/deb/rpm), arm64 (AppImage/deb/rpm) |

## البدء

توجه إلى صفحة [Releases](https://github.com/AQBot-Desktop/AQBot/releases) وقم بتنزيل المثبّت الخاص بمنصتك.

## الأسئلة الشائعة

### macOS: «التطبيق تالف» أو «لا يمكن التحقق من المطور»

نظراً لأن التطبيق غير موقّع من Apple، قد يعرض macOS أحد الرسائل التالية:

- «AQBot» تالف ولا يمكن فتحه
- لا يمكن فتح «AQBot» لأن Apple لا تستطيع التحقق منه بحثاً عن البرامج الضارة

**خطوات الحل:**

**1. السماح بالتطبيقات من «أي مكان»**

```bash
sudo spctl --master-disable
```

ثم انتقل إلى **إعدادات النظام ← الخصوصية والأمان ← الأمان** وحدد **أي مكان**.

**2. إزالة سمة الحجر الصحي**

```bash
sudo xattr -dr com.apple.quarantine /Applications/AQBot.app
```

> تلميح: يمكنك سحب أيقونة التطبيق إلى الطرفية بعد كتابة `sudo xattr -dr com.apple.quarantine `.

**3. خطوة إضافية لـ macOS Ventura والإصدارات الأحدث**

بعد إتمام الخطوات أعلاه، قد يظل الإطلاق الأول محجوباً. انتقل إلى **إعدادات النظام ← الخصوصية والأمان**، ثم انقر على **فتح على أي حال** في قسم الأمان. هذا يحتاج إلى تنفيذه مرة واحدة فقط.

## المجتمع
- [LinuxDO](https://linux.do)

## الترخيص

هذا المشروع مرخّص بموجب ترخيص [AGPL-3.0](LICENSE).
