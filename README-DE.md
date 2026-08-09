[简体中文](./README.md) | [繁體中文](./README-ZH-TW.md) | [English](./README-EN.md) | [日本語](./README-JA.md) | [한국어](./README-KO.md) | [Français](./README-FR.md) | **Deutsch** | [Español](./README-ES.md) | [Русский](./README-RU.md) | [हिन्दी](./README-HI.md) | [العربية](./README-AR.md)

[![AQBot](https://socialify.git.ci/AQBot-Desktop/AQBot/image?description=1&font=JetBrains+Mono&forks=1&issues=1&logo=https%3A%2F%2Fgithub.com%2FAQBot-Desktop%2FAQBot%2Fblob%2Fmain%2Fsrc%2Fassets%2Fimage%2Flogo.png%3Fraw%3Dtrue&name=1&owner=1&pattern=Floating+Cogs&pulls=1&stargazers=1&theme=Auto)](https://github.com/AQBot-Desktop/AQBot)

AQBot ist ein lokal ausgerichteter KI-Arbeitsbereich für den Desktop, der Chats mit mehreren Anbietern, ACP-Agents, Wissensdatenbanken, MCP-Werkzeuge und ein API-Gateway vereint, während App-Daten und Benutzerdateien unter deiner Kontrolle bleiben.

## Screenshots

| Chat-Diagramm-Rendering | Anbieter und Modelle |
|:---:|:---:|
| ![](.github/images/s1-0412.png) | ![](.github/images/s2-0412.png) |

| Wissensdatenbank | Gedächtnis |
|:---:|:---:|
| ![](.github/images/s3-0412.png) | ![](.github/images/s4-0412.png) |

| Agent - Anfrage | API-Gateway Ein-Klick-Zugang |
|:---:|:---:|
| ![](.github/images/s5-0412.png) | ![](.github/images/s6-0412.png) |

| Chat-Modell-Auswahl | Chat-Navigation |
|:---:|:---:|
| ![](.github/images/s7-0412.png) | ![](.github/images/s8-0412.png) |

| Agent - Berechtigungsgenehmigung | API-Gateway-Übersicht |
|:---:|:---:|
| ![](.github/images/s9-0412.png) | ![](.github/images/s10-0412.png) |

## Funktionen

### Chat & Modelle

- **Multi-Provider-Chat** — OpenAI, Claude, Gemini, DeepSeek, Qwen und OpenAI-kompatible Endpunkte mit Base URL, API Path, Headers und Proxy-Regeln verbinden.
- **Provider-Onboarding** — aqbot:// Provider-Links und CC Switch-Import übernehmen Provider-Profile nach Benutzerbestätigung.
- **Modellverwaltung** — Remote-Modelle synchronisieren, Gruppen, Latenz, Fähigkeiten, Kontextlänge, Sampling, Reasoning-Profile und extra_body pro Modell konfigurieren.
- **Gesprächs-Workflows** — Streaming, Denkblöcke, Nachrichtenversionen, Branches, Titelstatus, Komprimierung und Multi-Modell-Vergleich.

### AI Agent

- **Zwei Arten, Agents zu nutzen** — AQBot bietet sowohl einen in den Chat integrierten Agent als auch einen separaten ACP-Agent-Arbeitsbereich. Der erste nutzt deine konfigurierten Anbieter-APIs, der zweite verbindet sich mit externen ACP-kompatiblen Agent-Prozessen – passend zu Modell und Workflow.
- **Chat-Agent (Anbieter-API)** — Schalte eine normale Unterhaltung in den Agent-Modus und nutze die konfigurierte Anbieter- und Modell-API direkt, um in einem isolierten Arbeitsverzeichnis Dateien zu lesen oder zu bearbeiten, Befehle auszuführen und Code zu analysieren.
- **Steuerung des Chat-Agents** — Wähle Berechtigungsmodi wie jedes Mal fragen, Änderungen akzeptieren oder Vollzugriff, prüfe Werkzeugaufrufe und Freigaben in Echtzeit und verfolge Tokens und Kosten jeder Ausführung.
- **ACP-Agent-Arbeitsbereich** — Führe kompatible Coding-Agents über das [Agent Client Protocol (ACP)](https://agentclientprotocol.com/) in einem eigenen Arbeitsbereich aus und verfolge Antworten, Schlussfolgerungen und Werkzeugaufrufe als Stream.
- **ACP Registry und eigene Integrationen** — Füge Codex, Claude Agent, Gemini CLI, Cline, OpenCode, Grok Build und weitere Agents aus der Registry hinzu oder konfiguriere eigene Befehle, Argumente, Umgebungsvariablen und Symbole; Agents lassen sich frei aktivieren und sortieren.
- **ACP-Projekte und direkte Chats** — Organisiere Threads nach Projekt, hefte wichtige Sitzungen an und stelle den letzten Arbeitsstand automatisch wieder her, oder starte ohne Projektauswahl in einem isolierten Arbeitsverzeichnis.
- **Vollständige ACP-Sitzungsinteraktion** — Wechsle Modelle, Modi und Optionen des jeweiligen Agents und nutze Anhänge, Fragebögen, Planprüfungen, Fortschrittsanzeigen sowie die Wiederherstellung des Zustands nach dem Neuladen; Werkzeuganfragen lassen sich einmalig oder immer für die aktuelle Sitzung erlauben.

### Rollen

- **Lokale Rollenverwaltung** — System Prompts, Avatare, Tags, Begrüßungen, Startfragen, Temperatur und Top P als wiederverwendbare Gesprächsvorlagen speichern.
- **Ein-Klick-Nutzung** — Standardmäßig eine neue Rollenunterhaltung starten oder die Rolle per Dropdown auf die aktuelle Unterhaltung anwenden; Rollenunterhaltungen behalten Name, Avatar und blaues Rollen-Badge.
- **Marketplace** — Rollen aus prompts.chat und PlexPt 中文 suchen, installieren und lokal verwenden.

### Skills-Verwaltung

- **Multi-Source-Skill-Verzeichnisse** — AQBot-, Codex-, Claude- und Agents-Skill-Roots verwalten, inklusive `~/.aqbot/skills`, `~/.codex/skills`, `~/.claude/skills` und `~/.agents/skills`.
- **Meine Skills** — Nach Quelle filtern, aktivieren/deaktivieren, Details ansehen, Namen kopieren, Verzeichnis öffnen und deinstallieren.
- **Skill-Gruppen und Installationsziele** — Skills per group einklappen, gesammelt aktivieren/deaktivieren, Gruppenordner öffnen, ganze Gruppen deinstallieren und aus `owner/repo` oder GitHub-URLs in ein Ziel installieren.
- **Marketplace** — skills.sh- und GitHub-Quellen durchsuchen, Details ansehen, zu GitHub wechseln und Installationsstatus sehen.

### Inhaltsrendering

- **Markdown und Mathematik** — Markdown, Code, Tabellen, Aufgabenlisten und LaTeX in Streaming-Gesprächen rendern.
- **Code, Diagramme, Artifacts** — Monaco, Mermaid, D2 und Artifact-Panel für Code, Markdown-Notizen, Reports und Vorschauen.
- **HTML-Fragmente** — Generierte HTML-Fragmente sicher anzeigen, inklusive aktueller Streaming-Stabilitätsfixes.

### Suche & Wissen

- **Websuche** — Tavily, Exa, Zhipu WebSearch, Bocha mit Quellenangaben und Suchquery-Generierung.
- **Lokale Wissensbasis** — Private Dokumente mit sqlite-vec indexieren, Retrieval/Rerank konfigurieren und Feedback prüfen.
- **Kontextverwaltung** — Dateien, Suchergebnisse, Wissensausschnitte, Erinnerungen und Tool-Ausgaben anhängen.

### Werkzeuge & Erweiterungen

- **MCP-Protokoll** — Model Context Protocol-Server über stdio, SSE oder StreamableHTTP ausführen.
- **Integrierte Tools** — @aqbot/fetch und Dateisuche ohne separaten Server nutzen.
- **Tool-Loop-Limit** — Maximale MCP Tool-Call-Schleifen konfigurieren und blockierte Sessions besser wiederherstellen.

### API-Gateway

- **Lokales Gateway** — OpenAI Chat Completions, OpenAI Responses, Claude-native und Gemini-native Endpoints lokal bereitstellen.
- **Zugriff und Beobachtung** — Gateway-Schlüssel, SSL/TLS, Request-Logs und Nutzungsanalysen lokal verwalten.
- **Client-Templates** — Vorlagen für Claude Code, Codex CLI, OpenCode, Gemini CLI und Custom Clients.

### Datenimport & Backup

- **Drittanbieter-Importe** — ChatGPT, Cherry Studio und Kelivo mit Vorschau, Warnungen und Duplikatbehandlung importieren.
- **Provider- und Dateimigration** — Cherry Studio/Kelivo können Provider, API Keys und Anhänge optional migrieren.
- **Backups** — Backup/Restore über lokale Ordner, WebDAV oder S3-kompatiblen Speicher.

### Desktop & Sicherheit

- **Lokale Verschlüsselung** — App-Status in ~/.aqbot/, Benutzerdateien in ~/Documents/aqbot/, API Keys mit AES-256 geschützt.
- **Desktop-Integration** — Tray, Always-on-top, globale Shortcuts, Auto-Start, Proxy und Update-Prüfung.
- **11 UI-Sprachen** — Umschalten zwischen Chinesisch, Englisch, Japanisch, Koreanisch, Französisch, Deutsch, Spanisch, Russisch, Hindi und Arabisch.

## Plattformunterstützung

| Plattform | Architektur |
|-----------|------------|
| macOS | Apple Silicon (arm64), Intel (x86_64) |
| Windows 10/11 | x86_64, arm64 |
| Linux | x86_64 (AppImage/deb/rpm), arm64 (AppImage/deb/rpm) |

## Erste Schritte

Gehen Sie zur [Releases](https://github.com/AQBot-Desktop/AQBot/releases)-Seite und laden Sie das Installationsprogramm für Ihre Plattform herunter.

## FAQ

### macOS: „App ist beschädigt" oder „Entwickler kann nicht überprüft werden"

Da die Anwendung nicht von Apple signiert ist, kann macOS eine der folgenden Meldungen anzeigen:

- „AQBot" ist beschädigt und kann nicht geöffnet werden
- „AQBot" kann nicht geöffnet werden, da Apple es nicht auf Schadsoftware überprüfen kann

**Lösungsschritte:**

**1. Apps aus „Beliebiger Herkunft" zulassen**

```bash
sudo spctl --master-disable
```

Gehen Sie dann zu **Systemeinstellungen → Datenschutz & Sicherheit → Sicherheit** und wählen Sie **Beliebige Herkunft**.

**2. Das Quarantäne-Attribut entfernen**

```bash
sudo xattr -dr com.apple.quarantine /Applications/AQBot.app
```

> Tipp: Sie können das App-Symbol in das Terminal ziehen, nachdem Sie `sudo xattr -dr com.apple.quarantine ` eingegeben haben.

**3. Zusätzlicher Schritt für macOS Ventura und höher**

Nach Abschluss der obigen Schritte kann der erste Start immer noch blockiert werden. Gehen Sie zu **Systemeinstellungen → Datenschutz & Sicherheit** und klicken Sie im Sicherheitsbereich auf **Trotzdem öffnen**. Dies muss nur einmal durchgeführt werden.

## Community
- [LinuxDO](https://linux.do)

## Lizenz

Dieses Projekt ist unter der [AGPL-3.0](LICENSE)-Lizenz lizenziert.
