# Dextra

[Dextra source and releases](https://hm.ziqi.ac.cn:9400/zzq/dextra) · [Upstream documentation](https://docs.codeg.app) · [License](../../LICENSE)

<p>
  <a href="../../README.md">English</a> |
  <a href="./README.zh-CN.md">简体中文</a> |
  <a href="./README.zh-TW.md">繁體中文</a> |
  <strong>日本語</strong> |
  <a href="./README.ko.md">한국어</a> |
  <a href="./README.es.md">Español</a> |
  <a href="./README.de.md">Deutsch</a> |
  <a href="./README.fr.md">Français</a> |
  <a href="./README.pt.md">Português</a> |
  <a href="./README.ar.md">العربية</a>
</p>

Dextraはマルチエージェント・コーディングワークスペースです。あらゆる AI コーディングエージェントをひとつの場所で動かし、そして協働させます。

Dextra は Web サービスを内蔵したデスクトップアプリです。上流の iOS・Android クライアントは URL と Token を指定して接続できます。

![ワークスペース](../images/workspace-light.png#gh-light-mode-only)
![ワークスペース](../images/workspace-dark.png#gh-dark-mode-only)

## 📖 ドキュメント

**上流のドキュメント：** [docs.codeg.app](https://docs.codeg.app)。一部の機能は Dextra と異なる場合があります。

## 🤖 対応エージェント

Claude Code · Codex · Gemini · OpenClaw · OpenCode · Cline · Hermes · CodeBuddy · Kimi Code · Pi · Grok · Cursor · DeepSeek Harness · Qoder · Google Antigravity

その多くは Dextra がインストール・バージョン固定・更新まで面倒を見ます。全リスト、各エージェントの実行環境要件、セッションの保存場所は [対応エージェント](https://docs.codeg.app/guide/supported-agents) を参照してください。

リストにない？自分で追加できます。公開されている ACP レジストリから選ぶか、distribution JSON を貼り付けるだけで、Dextra がインストールし、起動できるかを事前に確認し、あとは内蔵エージェントと同じように扱います — ピッカーに並び、`@` 委譲やスキルにも対応し、そのエージェント自身が履歴を残さない場合でも会話は記録され検索できます。→ [カスタムエージェント](https://docs.codeg.app/guide/custom-agents)

## 🤝 マルチエージェント協調

マルチエージェント協調は、キーひとつで完結します。`@` を打ち、エージェントを選び、送信するだけ。あとのスケジューリングは Dextra が引き受けます — 指名されたエージェントをそれぞれ独立したセッションとして起動し、タスクを引き渡し、その作業を今いるスレッドへ流し込みます。ふたつ指名すれば並走します。Claude Code が下書きし、Codex がレビューする。コンテキストの切り替えも、ターミナル間のコピー＆ペーストも不要です。

エージェントが自前のサブエージェントを立ち上げたとき — Claude Code も Codex も Grok も OpenCode もそうします — 子ごとにカードができ、終わってからまとめて出るのではなく、動いている間に中身が埋まっていきます。開けば子自身のセッションを読めます。

![ひとつの Dextra 会話からサブエージェントへタスクを委譲する様子](../images/collaboration-light.gif#gh-light-mode-only)
![ひとつの Dextra 会話からサブエージェントへタスクを委譲する様子](../images/collaboration-dark.gif#gh-dark-mode-only)

## ✅ ToDo タスク

すべての仕事に付き添う必要はありません。書き留めるだけ — タイトル、説明、どのエージェントで走らせるか — で、Dextra がそれに**コードの専用コピー**を渡します。プロジェクトの隣に作られる git worktree で、専用のブランチの上です。同時にいくつ走っても互いに触れず、あなたが作業中のツリーにも触れません。今夜に予約することも、フォルダーに同時実行数の上限までキューを自分で消化させることもできます。

終わったタスクが自分でマージすることはありません。レビュー列へ移って待ちます。diff を読み、もう一周やり直させ、あるいは受け入れる — すると取り込むのはエージェントで、まずベースブランチを自分の worktree に取り込み、そこでコンフリクトを解消します。そのあと Dextra はエージェントの言い分ではなく git を確かめます。確認できなかったマージは、成功と報告される代わりにレビューへ戻ります。

![ToDo タスクのボード。タスクが「ToDo」から「進行中」を経て「完了」へ進む](../images/task-light.png#gh-light-mode-only)
![ToDo タスクのボード。タスクが「ToDo」から「進行中」を経て「完了」へ進む](../images/task-dark.png#gh-dark-mode-only)

## 🪟 画面分割

タブ列がひとつでは足りないときもあります。会話タブを右クリックすれば、ビューを**右**または**下**へ、何度でも分割できます — 左右に 2 つ、縦に 3 つ、あるいは格子状に。どのグループもそれ自体がひとつのワークスペースで、独自のタブ、独自のヘッダー、独自の新規会話ボタンを持ちます。片方のペインで Claude Code にリファクタリングさせ、隣のペインで Codex に diff をレビューさせる、という具合です。

タブをグループ間でドラッグしても、そのセッションは移動中もストリーミングを続けます。グループの境界をドラッグすれば、スペースの分け方が変わります。レイアウトはワークスペースごとに、下書きも含めて記憶されます — Dextra を開き直せば分割はそのまま戻り、送らなかった文字も入力欄に残っています。

![会話エリアをタブグループの格子に分割する](../images/split-light.gif#gh-light-mode-only)
![会話エリアをタブグループの格子に分割する](../images/split-dark.gif#gh-dark-mode-only)

## 📄 Office ドキュメント

スライドでも、レポートでも、表計算でも、頼めばエージェントは本物の `.pptx` / `.docx` / `.xlsx` を作ります — 右側のペインがそれをリアルタイムに描画しながら。編集は自動でプレビューへ反映され、スライドが埋まり、表が形になり、数値がセルに収まっていきます。4 枚目が気に入らない？次のメッセージでそう伝えるだけ — エージェントは同じファイルをその場で直し、プレビューが追いつきます。書き出しも、外部の Office アプリも、Dextra を離れる必要もありません。

![ライブプレビューを横に置いて Office ドキュメントを編集するエージェント](../images/office-light.png#gh-light-mode-only)
![ライブプレビューを横に置いて Office ドキュメントを編集するエージェント](../images/office-dark.png#gh-dark-mode-only)

## 💻 ワークスペース

ワークスペースはひとつ、エージェントはすべて。動かしているのが Claude Code でも Codex でも Cursor でも、同じエディタ、同じライブ diff、同じ Git クライアントの中で作業します。そして生まれるのはリポジトリの中の本物のファイル — 目の前で変わっていきます。別のディレクトリを取り込むこともできます — 共有ライブラリ、隣のサービス、ドキュメントのリポジトリ — ファイルツリーも検索もエージェント自身も、それらをひとつのワークスペースとして扱います。

**セッション**：すでにある履歴をそのまま引き継げます。インストール済みのすべてのエージェントの過去セッションをワンクリックで取り込み、中断したところから再開できます。取り込んだ後は、もう互いに孤立したままではありません — 古いセッションを `@` で指名すれば、いま話しているエージェントがそれを読めます。別のエージェントが書いたものでも構いません。今日の Codex が、先週の Claude Code が終えたところから続けられます。セッションがどれだけ長くなっても、開くときはまず直近のラウンドを表示し、残りはさかのぼるにつれて読み込みます。

**ファイル**：エージェントの編集は、着地するそばから会話の隣に diff として現れます。どのファイルもシンタックスハイライト付きの本物のエディタで開け、`⌘L` でファイルを — あるいは選択範囲だけを — そのままエージェントへ渡せます。Markdown、HTML、画像、Office ドキュメントも同じペインでプレビューできます。

**Git**：状態表示ではなく、完全なクライアントです。「変更」タブからそのままコミットでき — メッセージを書いて Enter — その隣に pull、fetch、push、stash が並び、履歴はどのコミットが push 済みかを示します。ブランチ、マージ、リベース、リセット、別ブランチとの差分に加えて、切り替えずに任意のブランチを更新・push できます。コンフリクトは三ペインのマージエディタで開き、ハンク単位で採用するか自分で書きます。そして worktree は並行作業をワンアクションに変えます — 新しいブランチ、専用のディレクトリ、そこに根を張った新しい会話。エージェントの一隊が互いのファイルに触れることなく、別々の機能を同時に作れます。

**うまくいかないとき**：失敗したターンは「何かが起きました」で終わりません。Claude Code と Codex では種類まで示します — 接続の問題、アクセスの問題、上限到達、リクエスト拒否、サービスの問題 — そして入力欄の下に、本当に役立つものだけを載せた帯が出ます。再試行、サインイン、あるいは新しいセッション。エージェントが自分で再試行している間は琥珀色で表示され、ターンが無事に終われば「復旧しました」の一行に収まります。入力欄の下の接続インジケーターもボタンです。押せばそのセッションの本当の状態が分かり、やり直しではなく再開する「再接続」もそこにあります。

## 📱 iPhone・iPad・Android

上流の iOS・Android クライアントは、URL と Token を使って Dextra の内蔵 Web サービスに接続できます。

| iPhone・iPad | Android |
| :---: | :---: |
| <img src="../images/mobile-ios.jpg" alt="iOS クライアントでセッションを開始する画面" width="248" /> | <img src="../images/mobile-android.jpg" alt="Android クライアントに流れ込むエージェントの返信" width="248" /> |

## ✨ ハイライト

- **[会話の集約](https://docs.codeg.app/guide/aggregation)** — 対応するすべてのエージェントのセッションを統一された検索可能なワークスペースへ取り込み、中断した続きから再開できます
- **[マルチエージェント協調](https://docs.codeg.app/guide/multi-agent)** — `@` でエージェントを指名するだけで委譲。異なる種類のサブエージェントがそれぞれ独立したセッションとして、ひとつのタスク内で並行して動きます
- **[ToDo タスク](https://docs.codeg.app/guide/tasks)** — やるべきことを書き留めればエージェントがキューを片づけていきます。各タスクは専用の worktree で走り、あなたがレビューして初めてブランチに取り込まれます
- **[カスタムエージェント](https://docs.codeg.app/guide/custom-agents)** — 公開レジストリまたは distribution JSON から、ACP 互換の任意のエージェントを登録。Dextra がインストールと履歴の記録を引き受け、内蔵エージェントと同じように扱います
- **[ワークスペース](https://docs.codeg.app/guide/workspace)** — エージェントの隣に開発の一連の流れがすべて揃います：ファイルツリー、エディタと diff、Git の変更、コミット、内蔵ターミナル、そして[ひとつのワークスペースにまとめた複数のフォルダー](https://docs.codeg.app/guide/workspace#work-across-several-folders)
- **[画面分割](https://docs.codeg.app/guide/workspace#split-the-conversation-view-into-groups)** — 会話エリアを好きな数のタブグループに分割し、タブや境界をドラッグして組み替え。再起動後もレイアウトは下書きごと復元します
- **[Git と Worktree](https://docs.codeg.app/guide/git)** — 変更のレビューとコミット、Git リモートアカウントの管理、内蔵の `git worktree` フローによる並行開発
- **[トークン使用量](https://docs.codeg.app/guide/token-usage)** — ステータスバーのカウンターの裏には完全なレポート：推移とキャッシュヒット率、アクティビティのヒートマップ、フォルダー・エージェント・モデル・セッション別の内訳
- **[チャットチャンネル](https://docs.codeg.app/guide/chat-channels)** — Telegram、Lark（飛書）、WeChat からエージェントを操作：タスク作成、権限の承認、進捗のリアルタイム受信
- **[オートメーション](https://docs.codeg.app/guide/automations)** — 設定済みの入力欄を再利用可能なオートメーションとして保存し、cron スケジュールまたは任意のタイミングでヘッドレス実行。セッションを開始することも、後であなたがレビューする ToDo タスクを積むこともできます
- **[Office ドキュメント](https://docs.codeg.app/guide/office)** — 同梱の `officecli` で `.docx` / `.xlsx` / `.pptx` を作成・分析・校正・編集し、タブ内でライブプレビュー
- **[科学研究](https://docs.codeg.app/guide/research)** — 同梱の研究スキル（仮説生成、実験計画、統計、可視化、批判的吟味、文献検索）をどのエージェントからも呼び出せます
- **[プロジェクトブート](https://docs.codeg.app/guide/project-boot)** — ライブプレビュー付きで新規プロジェクトを視覚的に構築し、そのままワークスペースで開きます
- **[MCP](https://docs.codeg.app/guide/mcp) & [スキル](https://docs.codeg.app/guide/skills)** — ローカルスキャンとレジストリ検索/インストール、スキルはグローバル／プロジェクト単位で管理
- **[自分の見た目に](https://docs.codeg.app/reference/settings/appearance)** — 12 のテーマをカラートークン単位で塗り替え、角丸をアプリ全体で設定し、テーマを shadcn JSON で読み書きし、必要なら自分で CSS を書けます
- **Dextra のデスクトップ Web サービス** — 許可されたブラウザーから内蔵サービスに接続できます。
- **[iPhone・iPad・Android](https://docs.codeg.app/getting-started/installation#mobile-apps)** — デスクトップやサーバーに接続するネイティブモバイルクライアント：どこからでもセッションを開始し、返信をストリーミングで受け取り、権限を承認し、プロジェクトを閲覧

## 📦 インストールと実行

**デスクトップ** — インストーラーの公開後は Convene のヘルプからダウンロードしてください。[Dextra のソース](https://hm.ziqi.ac.cn:9400/zzq/dextra) に現行クライアントのコードがあります。

**モバイル** — 上流の [iOS クライアント](https://github.com/xintaofei/codeg-ios) または [Android クライアント](https://github.com/xintaofei/codeg-android) を使い、Dextra Web サービスの URL と Token を入力してください。

## 🔒 プライバシーとセキュリティ

- 解析・保存・プロジェクト操作はデフォルトでローカル優先 — ネットワークアクセスはユーザーが起点となった操作でのみ発生します
- Web モードとサーバーモードはトークンベースの認証で保護されます
- 企業環境向けにシステムプロキシに対応

詳細は [プライバシーとセキュリティ](https://docs.codeg.app/reference/privacy) を参照してください。

## 👥 コミュニティ

ソースコードと更新情報は [Dextra リポジトリ](https://hm.ziqi.ac.cn:9400/zzq/dextra) を参照してください。

## 🙏 謝辞

- [Agent Client Protocol](https://agentclientprotocol.com) — Dextra が対応するすべてのエージェントへ接続できる土台
- [Superpowers](https://github.com/obra/superpowers) — Dextra のエキスパートスキルモジュールを支えるプロジェクト
- [OfficeCLI](https://github.com/iOfficeAI/OfficeCLI) — Dextra の Office ドキュメントワークフローを支えるプロジェクト
- [scientific-agent-skills](https://github.com/K-Dense-AI/scientific-agent-skills) — Dextra の科学研究スキルを支えるプロジェクト（MIT ライセンスのサブセット）

## 📜 ライセンス

Apache-2.0。[LICENSE](../../LICENSE) を参照してください。
