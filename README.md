# Dextra

[Source and releases](https://hm.ziqi.ac.cn:9400/zzq/dextra) · [License](./LICENSE)

<p>
  <strong>English</strong> |
  <a href="./docs/readme/README.zh-CN.md">简体中文</a> |
  <a href="./docs/readme/README.zh-TW.md">繁體中文</a> |
  <a href="./docs/readme/README.ja.md">日本語</a> |
  <a href="./docs/readme/README.ko.md">한국어</a> |
  <a href="./docs/readme/README.es.md">Español</a> |
  <a href="./docs/readme/README.de.md">Deutsch</a> |
  <a href="./docs/readme/README.fr.md">Français</a> |
  <a href="./docs/readme/README.pt.md">Português</a> |
  <a href="./docs/readme/README.ar.md">العربية</a>
</p>

Dextra is a multi-agent coding workspace: run every AI coding agent in one place — and let them work together.

Dextra is a desktop fork of [Codeg](https://github.com/xintaofei/codeg). It has its own app identity, files, settings and `dextra://` links. The upstream Codeg mobile clients can connect through the unchanged HTTP and WebSocket protocol. Links to `docs.codeg.app` below are upstream documentation and may differ from Dextra.

It aggregates your sessions from every supported agent CLI into one searchable workspace, and lets a main agent delegate to sub-agents of other types within a single task. Work you'd rather not sit through goes on a to-do board instead — each task in its own branch, running unattended, waiting for your review before it lands. Dextra is delivered as a desktop client with a built-in Web Service; fifteen agents come built in, and you can register any other ACP-compatible agent yourself.

![workspace](./docs/images/workspace-light.png#gh-light-mode-only)
![workspace](./docs/images/workspace-dark.png#gh-dark-mode-only)

## 📖 Documentation

The [upstream documentation](https://docs.codeg.app) covers shared features. Dextra installation and connection instructions are provided in Convene Help.

## 🤖 Supported Agents

Claude Code · Codex · Gemini · OpenClaw · OpenCode · Cline · Hermes · CodeBuddy · Kimi Code · Pi · Grok · Cursor · DeepSeek Harness · Qoder · Google Antigravity

Dextra installs, pins, and updates most of them for you. See [Supported Agents](https://docs.codeg.app/guide/supported-agents) for the full roster, each agent's runtime requirements, and where it keeps its sessions on disk.

Not on the list? Add it yourself. Pick any agent from the public ACP registry or paste its distribution JSON, and Dextra installs it, checks it can launch, and treats it like a built-in — it shows up in the picker, takes `@` delegation and skills, and gets its conversations recorded and searchable even when the agent keeps no history of its own. → [Custom Agents](https://docs.codeg.app/guide/custom-agents)

## 🤝 Multi-Agent Collaboration

Multi-agent collaboration, reduced to a single keystroke: type `@`, pick an agent, hit send. Dextra handles the scheduling — it launches each mentioned agent as its own session, hands over the task, and streams the work back into the thread you're already in. Mention two and they run side by side: Claude Code drafting while Codex reviews. No context switching, no copy-pasting between terminals.

And when an agent spawns sub-agents of its own — Claude Code, Codex, Grok and OpenCode all do — each child gets a card that fills in while it works, instead of landing all at once when it finishes. Open one to read the child's own session.

![Delegating a task to sub-agents from a single Dextra conversation](./docs/images/collaboration-light.gif#gh-light-mode-only)
![Delegating a task to sub-agents from a single Dextra conversation](./docs/images/collaboration-dark.gif#gh-dark-mode-only)

## ✅ To-dos

Not every job needs you watching it. Write one down — a title, a description, the agent to run it with — and Dextra hands it **its own copy of the code**: a git worktree beside your project, on its own branch. Several run at once without touching each other, or the tree you're working in. Schedule one for tonight, or let a folder work through its queue on its own, up to a concurrency limit you set.

A finished task doesn't merge itself. It moves to a review column and waits: read the diff, send it back for another pass, or accept it — and the agent lands it, pulling your base branch into its worktree and resolving conflicts there first. Dextra then checks git rather than taking the agent's word for it; a merge it can't confirm goes back to review instead of reporting success.

![The To-dos board, with tasks moving from To do through In progress to Done](./docs/images/task-light.png#gh-light-mode-only)
![The To-dos board, with tasks moving from To do through In progress to Done](./docs/images/task-dark.png#gh-dark-mode-only)

## 🪟 Split View

One tab strip isn't always enough. Right-click a conversation tab to split the view **right** or **down**, as many times as you like: two panes side by side, a stack of three, a grid. Each group is a workspace of its own — its own tabs, its own header, its own new-conversation button — so Claude Code can refactor in one pane while Codex reviews a diff in the next.

Drag a tab from one group into another and its session keeps streaming through the move; drag the divider between two groups to change how they share the space. Your layout is remembered per workspace, drafts included: reopen Dextra and the split comes back, with the text you never sent still in the box.

![Splitting the conversation area into a grid of tab groups](./docs/images/split-light.gif#gh-light-mode-only)
![Splitting the conversation area into a grid of tab groups](./docs/images/split-dark.gif#gh-dark-mode-only)

## 📄 Office Documents

Ask for a deck, a report, or a workbook and the agent builds a real `.pptx` / `.docx` / `.xlsx` — while the pane on the right renders it live. Every edit lands in the preview on its own: slides fill in, tables take shape, numbers land in cells. Don't like slide 4? Say so in the next message — the agent edits the same file in place and the preview catches up. No export step, no external Office app, no leaving Dextra.

![An agent editing an Office document beside its live in-tab preview](./docs/images/office-light.png#gh-light-mode-only)
![An agent editing an Office document beside its live in-tab preview](./docs/images/office-dark.png#gh-dark-mode-only)

## 💻 Workspace

One workspace, every agent. Whichever one is driving — Claude Code, Codex, Cursor — it works in the same editor, the same live diffs, the same git client, and what it produces is real files in your repo, changing while you watch. Link other directories in — a shared library, a sibling service, the docs repo — and the file tree, the search, and the agent itself treat them as one workspace.

**Sessions.** Pull in the history you already have: past sessions from every installed agent, imported in one click and resumable where you left them. Once they're in, they stop being separate silos — `@`-mention an old session and the agent you're talking to can read it, even when a different agent wrote it, so today's Codex run picks up where last week's Claude Code session ended. However long a thread gets, it opens on its recent rounds and pages the rest in as you scroll back.

**Files.** The agent's edits show up as diffs beside the conversation as they land. Open any file in a real editor with syntax highlighting, send a file — or just a selection — straight to the agent with `⌘L`, and preview Markdown, HTML, images, and Office documents in the same pane.

**Git.** A full client, not a status readout: commit straight from the Changes tab — type a message, press Enter — with pull, fetch, push and stash beside it, and history that shows which commits are pushed. Branch, merge, rebase, reset, or diff against another branch, and update or push any branch without switching to it. Conflicts open a three-pane merge editor where you accept hunk by hunk or type the fix yourself. And worktrees make parallel work one action — a new branch, its own directory, and a fresh conversation rooted in it, so a fleet of agents build different features at once without touching each other's files.

**When it goes wrong.** A failed turn doesn't just say something went wrong — on Claude Code and Codex it names the kind: a connection issue, an access issue, a limit reached, a request rejected, a service issue — and docks a strip under the composer carrying whatever would actually help, Retry or Sign in or a new session. Retries the agent makes on its own show amber and settle into a single "Recovered" line. And the connection indicator below the composer is a button: click it for the session's real state, with a Reconnect that resumes rather than starting over.

## 📱 iPhone, iPad & Android

The upstream Codeg iOS and Android clients can connect to Dextra's **Web Service** using its URL and Token. Dextra keeps the upstream Codeg mobile HTTP and WebSocket protocol unchanged. The phone clients are maintained upstream ([iOS](https://github.com/xintaofei/codeg-ios), [Android](https://github.com/xintaofei/codeg-android)); Dextra does not rebrand or package them.

|                                               iPhone & iPad                                               |                                                         Android                                                         |
| :-------------------------------------------------------------------------------------------------------: | :---------------------------------------------------------------------------------------------------------------------: |
| <img src="./docs/images/mobile-ios.jpg" alt="Starting a session from the Dextra iOS client" width="248" /> | <img src="./docs/images/mobile-android.jpg" alt="An agent reply streaming into the Dextra Android client" width="248" /> |

## ✨ Highlights

- **[Conversation Aggregation](https://docs.codeg.app/guide/aggregation)** — import sessions from every supported agent into one unified, searchable workspace, and pick any of them up where you left off
- **[Multi-Agent Collaboration](https://docs.codeg.app/guide/multi-agent)** — `@`-mention any agent to delegate: sub-agents of different types run as their own sessions, in parallel, inside a single task
- **[To-dos](https://docs.codeg.app/guide/tasks)** — write down what needs doing and agents work through the queue, each task in its own worktree, landing on your branch only after you've reviewed it
- **[Custom Agents](https://docs.codeg.app/guide/custom-agents)** — register any other ACP-compatible agent from the public registry or its distribution JSON; Dextra installs it, records its history, and treats it like a built-in
- **[The Workspace](https://docs.codeg.app/guide/workspace)** — the full engineering loop next to the agent: file tree, editor and diff, git changes, commit, an embedded terminal, and [several folders linked into one workspace](https://docs.codeg.app/guide/workspace#work-across-several-folders)
- **[URL scheme](docs/url-scheme.md)** — `dextra://session/<id>` opens a conversation from another app (desktop)
- **[Split View](https://docs.codeg.app/guide/workspace#split-the-conversation-view-into-groups)** — split the conversation area into as many tab groups as you like, drag tabs and dividers between them, and get the layout back — drafts included — on restart
- **[Git & Worktrees](https://docs.codeg.app/guide/git)** — review and commit changes, manage Git remote accounts, and run work in parallel with built-in `git worktree` flows
- **[Token Usage](https://docs.codeg.app/guide/token-usage)** — a full report behind the status-bar counter: trends and cache hit rate, an activity heatmap, and breakdowns by folder, agent, model, and session
- **[Chat Channels](https://docs.codeg.app/guide/chat-channels)** — drive your agents from Telegram, Lark (Feishu), and WeChat: create tasks, approve permissions, and get live updates
- **[Automations](https://docs.codeg.app/guide/automations)** — save a fully-configured composer as a reusable automation that runs headlessly, on a cron schedule or on demand — starting a session, or filing a to-do for you to review later
- **[Office Documents](https://docs.codeg.app/guide/office)** — create, analyze, proofread, and edit `.docx` / `.xlsx` / `.pptx` through the bundled `officecli`, with live in-tab preview
- **[Scientific Research](https://docs.codeg.app/guide/research)** — bundled research skills (hypothesis generation, experimental design, statistics, visualization, critical appraisal, literature search) any agent can invoke
- **[Project Boot](https://docs.codeg.app/guide/project-boot)** — scaffold new projects visually, with live preview, then open them straight in the workspace
- **[MCP](https://docs.codeg.app/guide/mcp) & [Skills](https://docs.codeg.app/guide/skills)** — local server scan plus registry search/install, and skills managed at global or project scope
- **[Make it yours](https://docs.codeg.app/reference/settings/appearance)** — recolor any of the twelve themes token by token, set the corner radius app-wide, import and export themes as shadcn JSON, or write your own CSS
- **Desktop Web Service** — a native desktop app whose built-in web service lets authorized browsers connect
- **[iPhone, iPad & Android](https://docs.codeg.app/getting-started/installation#mobile-apps)** — native mobile clients that connect to your desktop or server: start sessions, stream replies, approve permissions, and browse projects from anywhere

## 📦 Install & Run

**Desktop** — download Dextra packages from Convene Help when published. The [Dextra source repository](https://hm.ziqi.ac.cn:9400/zzq/dextra) contains the current client code.

**Mobile** — install an upstream [iOS client](https://github.com/xintaofei/codeg-ios) or [Android client](https://github.com/xintaofei/codeg-android), then configure the Dextra desktop Web Service URL and Token manually.

## 🔒 Privacy & Security

- Local-first by default for parsing, storage, and project operations — network access happens only on user-triggered actions
- Web and server modes are guarded by token-based authentication
- System proxy support for enterprise environments

Details in [Privacy & Security](https://docs.codeg.app/reference/privacy).

## 👥 Community

- Scan the QR code below to join our WeChat group for discussions, feedback, and updates

<img src="./docs/images/weixin-light.jpg#gh-light-mode-only" alt="WeChat" width="240" />
<img src="./docs/images/weixin-dark.jpg#gh-dark-mode-only" alt="WeChat" width="240" />

- Thanks to the [LinuxDO](https://linux.do) community for their support

## 🙏 Acknowledgments

- [Agent Client Protocol](https://agentclientprotocol.com) — the foundation that lets Dextra connect to every agent it supports
- [Superpowers](https://github.com/obra/superpowers) — powers Dextra's expert skills module
- [OfficeCLI](https://github.com/iOfficeAI/OfficeCLI) — powers Dextra's Office documents workflow
- [scientific-agent-skills](https://github.com/K-Dense-AI/scientific-agent-skills) — powers Dextra's Scientific Research skills (MIT-licensed subset)

## 📜 License

Apache-2.0. See [LICENSE](./LICENSE).
