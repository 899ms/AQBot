[简体中文](./README.md) | [繁體中文](./README-ZH-TW.md) | [English](./README-EN.md) | **日本語** | [한국어](./README-KO.md) | [Français](./README-FR.md) | [Deutsch](./README-DE.md) | [Español](./README-ES.md) | [Русский](./README-RU.md) | [हिन्दी](./README-HI.md) | [العربية](./README-AR.md)

[![AQBot](https://socialify.git.ci/AQBot-Desktop/AQBot/image?description=1&font=JetBrains+Mono&forks=1&issues=1&logo=https%3A%2F%2Fgithub.com%2FAQBot-Desktop%2FAQBot%2Fblob%2Fmain%2Fsrc%2Fassets%2Fimage%2Flogo.png%3Fraw%3Dtrue&name=1&owner=1&pattern=Floating+Cogs&pulls=1&stargazers=1&theme=Auto)](https://github.com/AQBot-Desktop/AQBot)

AQBot は、複数プロバイダーのチャット、ACP Agent、ナレッジベース、MCP ツール、API ゲートウェイを統合し、アプリデータとユーザーファイルを手元で管理できるローカルファーストのデスクトップ AI ワークスペースです。

## スクリーンショット

| チャットチャートレンダリング | プロバイダーとモデル |
|:---:|:---:|
| ![](.github/images/s1-0412.png) | ![](.github/images/s2-0412.png) |

| ナレッジベース | メモリー |
|:---:|:---:|
| ![](.github/images/s3-0412.png) | ![](.github/images/s4-0412.png) |

| Agent - 質問 | APIゲートウェイ ワンクリック接続 |
|:---:|:---:|
| ![](.github/images/s5-0412.png) | ![](.github/images/s6-0412.png) |

| チャットモデル選択 | チャットナビゲーション |
|:---:|:---:|
| ![](.github/images/s7-0412.png) | ![](.github/images/s8-0412.png) |

| Agent - 権限承認 | APIゲートウェイ概要 |
|:---:|:---:|
| ![](.github/images/s9-0412.png) | ![](.github/images/s10-0412.png) |

## 機能一覧

### チャットとモデル

- **マルチプロバイダー対応** — OpenAI、Claude、Gemini、DeepSeek、Qwen、OpenAI 互換エンドポイントを、Base URL、API Path、ヘッダー、プロキシ設定付きで接続できます。
- **プロバイダー導入** — aqbot:// プロバイダーリンクと CC Switch インポートで、ユーザー確認後にプロバイダー設定を取り込めます。
- **モデル管理** — リモートモデル同期、グループ管理、レイテンシーテスト、能力タグ、コンテキスト長、サンプリング既定値、推論プロファイル、モデル別 extra_body を設定できます。
- **会話ワークフロー** — ストリーミング返信、思考ブロック、メッセージバージョン、会話分岐、タイトル生成状態、長文圧縮、複数モデル並列回答に対応します。

### AI Agent

- **2 つの Agent 利用方法** — AQBot にはチャット内蔵 Agent と独立した ACP Agent ワークベンチがあります。前者は設定済みプロバイダーの API を使用し、後者は ACP 対応の外部 Agent プロセスへ接続するため、モデルやワークフローに応じて選択できます。
- **チャット Agent（プロバイダー API）** — 通常の会話を Agent モードに切り替え、設定済みのプロバイダーとモデル API をそのまま使って、分離された作業ディレクトリ内のファイル編集、コマンド実行、コード分析を行います。
- **チャット Agent の制御** — 毎回確認、編集を許可、フルアクセスなどの権限モードを選択し、ツール呼び出しと承認をリアルタイムで確認できます。実行ごとの token とコストも記録されます。
- **ACP Agent ワークベンチ** — [Agent Client Protocol (ACP)](https://agentclientprotocol.com/) 対応のコーディング Agent を専用画面で実行し、応答、推論、ツール呼び出しをストリーミング表示します。
- **ACP Registry とカスタム連携** — Registry から Codex、Claude Agent、Gemini CLI、Cline、OpenCode、Grok Build などを追加できるほか、独自のコマンド、引数、環境変数、アイコンを設定し、Agent の有効化や並べ替えも行えます。
- **ACP プロジェクトとダイレクトチャット** — スレッドをプロジェクト別に整理し、重要なセッションをピン留めして前回の作業状態を自動復元できます。プロジェクトを選ばず、分離された作業ディレクトリで直接チャットを始めることもできます。
- **充実した ACP セッション操作** — 各 Agent が公開するモデル、モード、設定の切り替えに加え、添付ファイル、アンケート、計画レビュー、進捗表示、再読み込み後の状態復元に対応します。ツール要求は今回のみ許可、または現在のセッションで「常に許可」を選択できます。

### ロール

- **ローカルロール管理** — system prompt、avatar、tag、opening message、starter questions、temperature、Top P を再利用可能な会話テンプレートとして保存します。
- **ワンクリック利用** — デフォルトでは新しいロール会話を作成し、ドロップダウンから現在の会話にも適用できます。ロール会話は名前、avatar、青いロールバッジを保持します。
- **オンラインマーケット** — prompts.chat と PlexPt 中文ソースからロールを検索・インストールし、ローカルで利用できます。

### Skills 管理

- **複数ソースの skills ディレクトリ** — AQBot、Codex、Claude、Agents の skills ルートを管理します。`~/.aqbot/skills`、`~/.codex/skills`、`~/.claude/skills`、`~/.agents/skills` に対応します。
- **My Skills** — ソース絞り込み、有効/無効、詳細表示、名前コピー、ディレクトリを開く、アンインストールに対応します。
- **Skill group とインストール先** — group 単位で折り畳み、まとめて有効/無効、グループディレクトリを開く、グループ削除ができ、`owner/repo` または GitHub URL から指定先へインストールできます。
- **Marketplace** — skills.sh と GitHub ソースの検索、詳細プレビュー、GitHub への移動、インストール済み状態を表示します。

### コンテンツレンダリング

- **Markdown と数式** — ストリーミング会話で Markdown、コードハイライト、表、タスクリスト、LaTeX 数式を表示します。
- **コード、図、Artifact** — Monaco コードブロック、Mermaid、D2、Artifact パネルでコード、Markdown メモ、レポート、プレビューを扱えます。
- **HTML フラグメント** — 生成された HTML 断片を安全にプレビューし、最近のリリースで追加されたストリーミング安定化も反映しています。

### 検索とナレッジ

- **Web 検索** — Tavily、Exa、Zhipu WebSearch、Bocha などを使い、引用元と検索クエリ生成を会話に追加できます。
- **ローカルナレッジベース** — sqlite-vec で非公開ドキュメントを索引化し、取得/リランク設定と検索フィードバックを確認できます。
- **コンテキスト管理** — ファイル、検索結果、ナレッジ断片、メモリ、ツール出力を会話コンテキストへ追加できます。

### ツールと拡張機能

- **MCP プロトコル** — stdio、SSE、StreamableHTTP の Model Context Protocol サーバーを実行できます。
- **ビルトインツール** — @aqbot/fetch やファイル検索などの内蔵 MCP ツールを、追加サーバーなしで利用できます。
- **ツールループ上限** — MCP ツール呼び出しの最大ループ数を設定し、中断や停止したツールセッションから復帰しやすくなりました。

### API ゲートウェイ

- **ローカルゲートウェイ** — デスクトップアプリから OpenAI Chat Completions、OpenAI Responses、Claude ネイティブ、Gemini ネイティブ API を公開します。
- **アクセスと可観測性** — ゲートウェイキー、SSL/TLS 証明書、リクエストログ、利用統計をローカルで管理できます。
- **クライアントテンプレート** — Claude Code、Codex CLI、OpenCode、Gemini CLI、カスタムクライアント向けのテンプレートを提供します。

### データインポートとバックアップ

- **サードパーティインポート** — ChatGPT 公式エクスポート、Cherry Studio、Kelivo バックアップをプレビュー、警告、重複処理付きで取り込めます。
- **プロバイダーとファイル移行** — Cherry Studio/Kelivo から関連プロバイダー、API キー、添付ファイルを任意で移行できます。
- **バックアップ** — ローカルフォルダー、WebDAV、S3 互換ストレージでバックアップと復元を行えます。

### デスクトップとセキュリティ

- **ローカル暗号化** — アプリ状態は ~/.aqbot/、ユーザーファイルは ~/Documents/aqbot/ に保存され、API キーは AES-256 とローカルマスターキーで保護されます。
- **デスクトップ統合** — トレイ、常に手前、グローバルショートカット、自動起動、プロキシ、自動更新チェックをサポートします。
- **11 言語 UI** — 簡体字中国語、繁体字中国語、英語、日本語、韓国語、フランス語、ドイツ語、スペイン語、ロシア語、ヒンディー語、アラビア語を切り替えられます。

## プラットフォームサポート

| プラットフォーム | アーキテクチャ |
|-----------------|---------------|
| macOS | Apple Silicon (arm64), Intel (x86_64) |
| Windows 10/11 | x86_64, arm64 |
| Linux | x86_64 (AppImage/deb/rpm), arm64 (AppImage/deb/rpm) |

## はじめに

[Releases](https://github.com/AQBot-Desktop/AQBot/releases) ページにアクセスして、お使いのプラットフォーム向けのインストーラーをダウンロードしてください。

## よくある質問

### macOS：「アプリが壊れています」または「開発元を確認できません」

アプリケーションが Apple によって署名されていないため、macOS は次のいずれかのプロンプトを表示する場合があります：

- 「AQBot」は壊れているため開けません
- 悪意のあるソフトウェアがないか確認できないため、「AQBot」を開けません

**解決手順：**

**1. 「すべてのアプリケーションを許可」する**

```bash
sudo spctl --master-disable
```

次に **「システム設定 → プライバシーとセキュリティ → セキュリティ」** に移動し、**「すべてのアプリケーションを許可」** を選択してください。

**2. 検疫属性を削除する**

```bash
sudo xattr -dr com.apple.quarantine /Applications/AQBot.app
```

> ヒント：ターミナルに `sudo xattr -dr com.apple.quarantine ` と入力した後、アプリアイコンをドラッグ＆ドロップできます。

**3. macOS Ventura 以降の追加手順**

上記の手順を完了した後も、初回起動時にブロックされる場合があります。**「システム設定 → プライバシーとセキュリティ」** に移動し、セキュリティセクションの **「このまま開く」** をクリックしてください。この操作は一度だけ必要です。

## コミュニティ
- [LinuxDO](https://linux.do)

## ライセンス

このプロジェクトは [AGPL-3.0](LICENSE) ライセンスの下でライセンスされています。
