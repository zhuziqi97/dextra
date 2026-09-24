# Dextra

[Dextra source and releases](https://hm.ziqi.ac.cn:9400/zzq/dextra) · [Upstream documentation](https://docs.codeg.app) · [License](../../LICENSE)

<p>
  <a href="../../README.md">English</a> |
  <a href="./README.zh-CN.md">简体中文</a> |
  <a href="./README.zh-TW.md">繁體中文</a> |
  <a href="./README.ja.md">日本語</a> |
  <a href="./README.ko.md">한국어</a> |
  <a href="./README.es.md">Español</a> |
  <strong>Deutsch</strong> |
  <a href="./README.fr.md">Français</a> |
  <a href="./README.pt.md">Português</a> |
  <a href="./README.ar.md">العربية</a>
</p>

Dextra ist ein Multi-Agent-Coding-Workspace: Führe jeden KI-Coding-Agenten an einem Ort aus — und lass sie zusammenarbeiten.

Dextra ist eine Desktop-App mit integriertem Webdienst. Die iOS- und Android-Clients des Ursprungsprojekts können sich per URL und Token verbinden.

![Workspace](../images/workspace-light.png#gh-light-mode-only)
![Workspace](../images/workspace-dark.png#gh-dark-mode-only)

## 📖 Dokumentation

**Dokumentation des Ursprungsprojekts:** [docs.codeg.app](https://docs.codeg.app). Einzelne Funktionen können von Dextra abweichen.

## 🤖 Unterstützte Agenten

Claude Code · Codex · Gemini · OpenClaw · OpenCode · Cline · Hermes · CodeBuddy · Kimi Code · Pi · Grok · Cursor · DeepSeek Harness · Qoder · Google Antigravity

Die meisten davon installiert, fixiert und aktualisiert Dextra für dich. Die vollständige Liste, die Laufzeit-Anforderungen jedes Agenten und den Ablageort seiner Sitzungen findest du unter [Unterstützte Agenten](https://docs.codeg.app/guide/supported-agents).

Nicht dabei? Füge ihn selbst hinzu. Wähle einen Agenten aus der öffentlichen ACP-Registry oder füge sein Distribution-JSON ein — Dextra installiert ihn, prüft vorab, ob er startet, und behandelt ihn wie einen eingebauten: Er erscheint im Picker, nimmt `@`-Delegation und Skills an, und seine Unterhaltungen werden aufgezeichnet und durchsuchbar, selbst wenn der Agent selbst keine Historie führt. → [Eigene Agenten](https://docs.codeg.app/guide/custom-agents)

## 🤝 Multi-Agent-Zusammenarbeit

Multi-Agent-Zusammenarbeit, reduziert auf einen Tastendruck: `@` tippen, Agenten auswählen, absenden. Um die Orchestrierung kümmert sich Dextra — es startet jeden erwähnten Agenten als eigene Sitzung, übergibt die Aufgabe und streamt die Arbeit zurück in den Thread, in dem du ohnehin bist. Erwähne zwei, und sie laufen nebeneinander: Claude Code schreibt, Codex prüft. Kein Kontextwechsel, kein Copy-and-paste zwischen Terminals.

Und wenn ein Agent eigene Sub-Agenten startet — Claude Code, Codex, Grok und OpenCode tun das alle — bekommt jedes Kind eine eigene Karte, die sich während der Arbeit füllt, statt erst am Ende auf einen Schlag zu erscheinen. Öffne eine, und du liest die Sitzung des Kindes selbst.

![Eine Aufgabe wird aus einer einzigen Dextra-Unterhaltung an Sub-Agenten delegiert](../images/collaboration-light.gif#gh-light-mode-only)
![Eine Aufgabe wird aus einer einzigen Dextra-Unterhaltung an Sub-Agenten delegiert](../images/collaboration-dark.gif#gh-dark-mode-only)

## ✅ To-dos

Nicht jede Arbeit braucht dich als Zuschauer. Schreib sie auf — ein Titel, eine Beschreibung, der Agent, der sie erledigen soll — und Dextra gibt ihr **eine eigene Kopie des Codes**: ein Git-Worktree neben deinem Projekt, auf einem eigenen Branch. Mehrere laufen gleichzeitig, ohne sich gegenseitig oder den Baum zu berühren, in dem du gerade arbeitest. Plane eine für heute Abend, oder lass einen Ordner seine Warteschlange selbst abarbeiten, bis zu dem Limit, das du festlegst.

Eine fertige Aufgabe merged sich nicht selbst. Sie wandert in die Review-Spalte und wartet: Diff lesen, für eine weitere Runde zurückschicken oder annehmen — und der Agent bringt sie ein, holt dafür erst deinen Base-Branch in sein Worktree und löst die Konflikte dort. Danach glaubt Dextra dem Agenten nicht aufs Wort, sondern prüft Git selbst: Ein Merge, den es nicht bestätigen kann, geht zurück ins Review, statt Erfolg zu melden.

![Das To-do-Board, auf dem Aufgaben von „To-do“ über „In Arbeit“ nach „Fertig“ wandern](../images/task-light.png#gh-light-mode-only)
![Das To-do-Board, auf dem Aufgaben von „To-do“ über „In Arbeit“ nach „Fertig“ wandern](../images/task-dark.png#gh-dark-mode-only)

## 🪟 Split-Ansicht

Eine Tab-Leiste reicht nicht immer. Ein Rechtsklick auf einen Unterhaltungs-Tab teilt die Ansicht **nach rechts** oder **nach unten** — beliebig oft: zwei Panes nebeneinander, drei gestapelt, ein Raster. Jede Gruppe ist ein vollwertiger Workspace — eigene Tabs, eigener Header, eigener Button für eine neue Unterhaltung — so refaktoriert Claude Code im einen Pane, während Codex im nächsten einen Diff prüft.

Zieh einen Tab von einer Gruppe in die andere: Seine Sitzung streamt während des Umzugs weiter. Zieh den Teiler zwischen zwei Gruppen, um den Platz anders aufzuteilen. Das Layout wird pro Workspace gemerkt, Entwürfe inklusive — öffne Dextra wieder, und die Aufteilung ist zurück, mit dem nie abgesendeten Text noch im Eingabefeld.

![Der Unterhaltungsbereich wird in ein Raster aus Tab-Gruppen geteilt](../images/split-light.gif#gh-light-mode-only)
![Der Unterhaltungsbereich wird in ein Raster aus Tab-Gruppen geteilt](../images/split-dark.gif#gh-dark-mode-only)

## 📄 Office-Dokumente

Bitte um ein Deck, einen Bericht oder eine Arbeitsmappe, und der Agent baut eine echte `.pptx` / `.docx` / `.xlsx` — während der rechte Bereich sie live rendert. Jede Änderung landet von selbst in der Vorschau: Folien füllen sich, Tabellen nehmen Gestalt an, Zahlen landen in den Zellen. Folie 4 gefällt nicht? Sag es in der nächsten Nachricht — der Agent bearbeitet dieselbe Datei an Ort und Stelle, die Vorschau zieht nach. Kein Export, keine externe Office-App, kein Verlassen von Dextra.

![Ein Agent bearbeitet ein Office-Dokument neben dessen Live-Vorschau](../images/office-light.png#gh-light-mode-only)
![Ein Agent bearbeitet ein Office-Dokument neben dessen Live-Vorschau](../images/office-dark.png#gh-dark-mode-only)

## 💻 Workspace

Ein Workspace, alle Agenten. Egal welcher gerade arbeitet — Claude Code, Codex, Cursor —, er tut es im selben Editor, mit denselben Live-Diffs und demselben Git-Client. Und was dabei entsteht, sind echte Dateien in deinem Repository, die sich vor deinen Augen verändern. Binde weitere Verzeichnisse ein — eine gemeinsame Bibliothek, einen Nachbardienst, das Docs-Repository — und Dateibaum, Suche und der Agent selbst behandeln sie als einen Workspace.

**Sitzungen.** Hol dir die Historie, die du schon hast: vergangene Sitzungen aller installierten Agenten, mit einem Klick importiert und dort fortsetzbar, wo du aufgehört hast. Einmal drin, bleiben sie keine getrennten Silos — erwähne eine alte Sitzung mit `@`, und der Agent, mit dem du gerade sprichst, kann sie lesen, auch wenn ein anderer Agent sie geschrieben hat. So macht der heutige Codex-Lauf da weiter, wo die Claude-Code-Sitzung von letzter Woche aufgehört hat. Wie lang ein Verlauf auch wird: Er öffnet sich mit den jüngsten Runden und lädt den Rest nach, während du nach oben scrollst.

**Dateien.** Die Änderungen des Agenten erscheinen als Diffs neben der Unterhaltung, sobald sie landen. Öffne jede Datei in einem echten Editor mit Syntaxhervorhebung, schick eine Datei — oder nur eine Auswahl — mit `⌘L` direkt an den Agenten, und zeig dir Markdown, HTML, Bilder und Office-Dokumente in derselben Ansicht in der Vorschau an.

**Git.** Ein vollwertiger Client, keine Statusanzeige: committe direkt aus dem Tab „Änderungen“ — Nachricht tippen, Enter — mit Pull, Fetch, Push und Stash daneben und einer Historie, die zeigt, welche Commits gepusht sind. Branches anlegen, mergen, rebasen, zurücksetzen oder gegen einen anderen Branch diffen — und jeden Branch aktualisieren oder pushen, ohne zu ihm zu wechseln. Konflikte öffnen einen dreispaltigen Merge-Editor, in dem du Hunk für Hunk übernimmst oder die Lösung selbst tippst. Und Worktrees machen paralleles Arbeiten zu einer einzigen Aktion — ein neuer Branch, ein eigenes Verzeichnis und eine frische Unterhaltung darin, sodass eine ganze Flotte von Agenten gleichzeitig an verschiedenen Features baut, ohne einander in die Quere zu kommen.

**Wenn etwas schiefgeht.** Ein gescheiterter Turn sagt nicht bloß, dass etwas schiefging — bei Claude Code und Codex nennt er die Art: ein Verbindungsproblem, ein Zugriffsproblem, ein erreichtes Limit, eine abgelehnte Anfrage, ein Dienstproblem — und legt unter den Composer einen Streifen mit dem, was wirklich hilft: Wiederholen, Anmelden oder eine neue Sitzung. Versuche, die der Agent von sich aus unternimmt, erscheinen bernsteinfarben und schrumpfen am Ende auf eine einzige Zeile „Wiederhergestellt“. Und die Verbindungsanzeige unter dem Composer ist ein Button: ein Klick zeigt den echten Zustand der Sitzung — samt einem Reconnect, das fortsetzt statt neu zu beginnen.

## 📱 iPhone, iPad & Android

Die iOS- und Android-Clients des Ursprungsprojekts verbinden sich per URL und Token mit dem integrierten Dextra-Webdienst.

| iPhone & iPad | Android |
| :---: | :---: |
| <img src="../images/mobile-ios.jpg" alt="Eine Sitzung wird im Dextra-iOS-Client gestartet" width="248" /> | <img src="../images/mobile-android.jpg" alt="Eine Agenten-Antwort, die live in den Dextra-Android-Client läuft" width="248" /> |

## ✨ Highlights

- **[Sitzungs-Aggregation](https://docs.codeg.app/guide/aggregation)** — importiert Sitzungen aller unterstützten Agenten in einen einheitlichen, durchsuchbaren Workspace — und du machst dort weiter, wo du aufgehört hast
- **[Multi-Agent-Zusammenarbeit](https://docs.codeg.app/guide/multi-agent)** — per `@`-Erwähnung an jeden Agenten delegieren: Sub-Agenten unterschiedlicher Typen laufen als eigene Sitzungen, parallel, innerhalb einer Aufgabe
- **[To-dos](https://docs.codeg.app/guide/tasks)** — schreib auf, was zu tun ist, und Agenten arbeiten die Warteschlange ab, jede Aufgabe in ihrem eigenen Worktree — auf deinem Branch landet sie erst, nachdem du sie geprüft hast
- **[Eigene Agenten](https://docs.codeg.app/guide/custom-agents)** — jeden weiteren ACP-kompatiblen Agenten aus der öffentlichen Registry oder per Distribution-JSON registrieren; Dextra installiert ihn, zeichnet seine Historie auf und behandelt ihn wie einen eingebauten
- **[Der Workspace](https://docs.codeg.app/guide/workspace)** — der komplette Engineering-Loop direkt neben dem Agenten: Dateibaum, Editor und Diff, Git-Änderungen, Commit, ein eingebettetes Terminal und [mehrere Ordner, zu einem Workspace verbunden](https://docs.codeg.app/guide/workspace#work-across-several-folders)
- **[Split-Ansicht](https://docs.codeg.app/guide/workspace#split-the-conversation-view-into-groups)** — den Unterhaltungsbereich in beliebig viele Tab-Gruppen teilen, Tabs und Teiler zwischen ihnen ziehen und das Layout — samt Entwürfen — nach dem Neustart zurückbekommen
- **[Git & Worktrees](https://docs.codeg.app/guide/git)** — Änderungen prüfen und committen, Git-Remote-Konten verwalten und mit integrierten `git worktree`-Abläufen parallel arbeiten
- **[Token-Nutzung](https://docs.codeg.app/guide/token-usage)** — hinter dem Zähler in der Statusleiste steckt ein vollständiger Bericht: Verläufe und Cache-Trefferquote, eine Aktivitäts-Heatmap und Aufschlüsselungen nach Ordner, Agent, Modell und Sitzung
- **[Chat-Kanäle](https://docs.codeg.app/guide/chat-channels)** — steuere deine Agenten aus Telegram, Lark (Feishu) und WeChat: Aufgaben anlegen, Berechtigungen freigeben, Live-Updates erhalten
- **[Automatisierungen](https://docs.codeg.app/guide/automations)** — einen fertig konfigurierten Composer als wiederverwendbare Automatisierung sichern und headless per Cron-Zeitplan oder auf Zuruf ausführen — sie startet eine Sitzung oder legt ein To-do an, das du später prüfst
- **[Office-Dokumente](https://docs.codeg.app/guide/office)** — `.docx` / `.xlsx` / `.pptx` mit dem mitgelieferten `officecli` erstellen, analysieren, korrigieren und bearbeiten — mit Live-Vorschau im Tab
- **[Wissenschaftliche Recherche](https://docs.codeg.app/guide/research)** — mitgelieferte Research-Skills (Hypothesenbildung, Versuchsplanung, Statistik, Visualisierung, kritische Bewertung, Literatursuche), die jeder Agent aufrufen kann
- **[Project Boot](https://docs.codeg.app/guide/project-boot)** — neue Projekte visuell aufsetzen, mit Live-Vorschau, und direkt im Workspace öffnen
- **[MCP](https://docs.codeg.app/guide/mcp) & [Skills](https://docs.codeg.app/guide/skills)** — lokaler Server-Scan plus Suche/Installation aus der Registry, Skills global oder pro Projekt verwaltet
- **[Mach es zu deinem](https://docs.codeg.app/reference/settings/appearance)** — jedes der zwölf Themes Token für Token umfärben, den Eckenradius app-weit setzen, Themes als shadcn-JSON importieren und exportieren oder eigenes CSS schreiben
- **Dextra-Desktop-Webdienst** — autorisierte Browser können sich mit dem integrierten Dienst verbinden.
- **[iPhone, iPad & Android](https://docs.codeg.app/getting-started/installation#mobile-apps)** — native Mobile-Clients, die sich mit deinem Desktop oder Server verbinden: Sitzungen starten, Antworten streamen, Berechtigungen freigeben und Projekte von überall durchsehen

## 📦 Installation & Betrieb

**Desktop** — lade die Installationspakete nach ihrer Veröffentlichung aus der Convene-Hilfe herunter. Der [Dextra-Quellcode](https://hm.ziqi.ac.cn:9400/zzq/dextra) enthält den aktuellen Client.

**Mobil** — verwende den [iOS-Client](https://github.com/xintaofei/codeg-ios) oder [Android-Client](https://github.com/xintaofei/codeg-android) des Ursprungsprojekts und trage URL und Token des Dextra-Webdiensts ein.

## 🔒 Datenschutz und Sicherheit

- Standardmäßig local-first bei Parsing, Speicherung und Projektoperationen — Netzwerkzugriffe passieren nur bei von dir ausgelösten Aktionen
- Web- und Server-Modus sind durch Token-basierte Authentifizierung geschützt
- System-Proxy-Unterstützung für Unternehmensumgebungen

Details unter [Datenschutz und Sicherheit](https://docs.codeg.app/reference/privacy).

## 👥 Community

Quellcode und Updates findest du im [Dextra-Repository](https://hm.ziqi.ac.cn:9400/zzq/dextra).

## 🙏 Danksagungen

- [Agent Client Protocol](https://agentclientprotocol.com) — die Grundlage, auf der Dextra sich mit jedem unterstützten Agenten verbindet
- [Superpowers](https://github.com/obra/superpowers) — unterstützt das Experten-Skills-Modul von Dextra
- [OfficeCLI](https://github.com/iOfficeAI/OfficeCLI) — unterstützt den Office-Dokument-Workflow von Dextra
- [scientific-agent-skills](https://github.com/K-Dense-AI/scientific-agent-skills) — unterstützt die wissenschaftlichen Forschungs-Skills von Dextra (MIT-lizenzierte Teilmenge)

## 📜 Lizenz

Apache-2.0. Siehe [LICENSE](../../LICENSE).
