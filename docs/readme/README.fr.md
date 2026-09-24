# Dextra

[Dextra source and releases](https://hm.ziqi.ac.cn:9400/zzq/dextra) · [Upstream documentation](https://docs.codeg.app) · [License](../../LICENSE)

<p>
  <a href="../../README.md">English</a> |
  <a href="./README.zh-CN.md">简体中文</a> |
  <a href="./README.zh-TW.md">繁體中文</a> |
  <a href="./README.ja.md">日本語</a> |
  <a href="./README.ko.md">한국어</a> |
  <a href="./README.es.md">Español</a> |
  <a href="./README.de.md">Deutsch</a> |
  <strong>Français</strong> |
  <a href="./README.pt.md">Português</a> |
  <a href="./README.ar.md">العربية</a>
</p>

Dextra est un espace de travail de programmation multi-agents : faites tourner tous vos agents de codage IA au même endroit — et laissez-les travailler ensemble.

Dextra est une application de bureau avec un service web intégré. Les clients iOS et Android du projet d’origine peuvent se connecter avec une URL et un jeton.

![Espace de travail](../images/workspace-light.png#gh-light-mode-only)
![Espace de travail](../images/workspace-dark.png#gh-dark-mode-only)

## 📖 Documentation

**Documentation du projet d’origine :** [docs.codeg.app](https://docs.codeg.app). Certaines fonctionnalités peuvent différer de Dextra.

## 🤖 Agents supportés

Claude Code · Codex · Gemini · OpenClaw · OpenCode · Cline · Hermes · CodeBuddy · Kimi Code · Pi · Grok · Cursor · DeepSeek Harness · Qoder · Google Antigravity

Dextra installe, épingle et met à jour la plupart d'entre eux pour vous. Voir [Agents supportés](https://docs.codeg.app/guide/supported-agents) pour la liste complète, les prérequis d'exécution de chacun et l'emplacement de ses sessions sur le disque.

Pas dans la liste ? Ajoutez-le vous-même. Choisissez un agent dans le registre public ACP ou collez son JSON de distribution : Dextra l'installe, vérifie qu'il démarre et le traite comme un agent intégré — il apparaît dans le sélecteur, accepte la délégation `@` et les skills, et ses conversations sont enregistrées et consultables même quand l'agent ne conserve aucun historique. → [Agents personnalisés](https://docs.codeg.app/guide/custom-agents)

## 🤝 Collaboration multi-agents

La collaboration multi-agents, réduite à une seule touche : tapez `@`, choisissez un agent, envoyez. Dextra s'occupe de l'orchestration — il lance chaque agent mentionné dans sa propre session, lui confie la tâche et renvoie son travail dans le fil où vous êtes déjà. Mentionnez-en deux et ils avancent côte à côte : Claude Code rédige pendant que Codex relit. Aucun changement de contexte, aucun copier-coller entre terminaux.

Et quand un agent lance ses propres sous-agents — Claude Code, Codex, Grok et OpenCode le font tous — chaque enfant a sa carte, qui se remplit pendant qu'il travaille au lieu d'arriver d'un bloc à la fin. Ouvrez-la pour lire la session de l'enfant lui-même.

![Délégation d'une tâche à des sous-agents depuis une seule conversation Dextra](../images/collaboration-light.gif#gh-light-mode-only)
![Délégation d'une tâche à des sous-agents depuis une seule conversation Dextra](../images/collaboration-dark.gif#gh-dark-mode-only)

## ✅ Tâches à faire

Tout travail n'exige pas que vous le regardiez. Notez-le — un titre, une description, l'agent qui doit l'exécuter — et Dextra lui confie **sa propre copie du code** : un worktree git à côté de votre projet, sur sa propre branche. Plusieurs tournent à la fois sans se toucher, ni toucher l'arbre dans lequel vous travaillez. Programmez-en une pour ce soir, ou laissez un dossier vider sa file tout seul, dans la limite de parallélisme que vous fixez.

Une tâche terminée ne se fusionne pas toute seule. Elle passe dans la colonne de relecture et attend : lisez le diff, renvoyez-la pour un tour de plus, ou acceptez-la — et c'est l'agent qui la fait atterrir, en ramenant d'abord votre branche de base dans son worktree et en y résolvant les conflits. Ensuite Dextra ne le croit pas sur parole : il vérifie git lui-même, et une fusion qu'il ne peut pas confirmer retourne en relecture au lieu d'annoncer un succès.

![Le tableau des tâches à faire, où les tâches passent de À faire à En cours puis à Terminé](../images/task-light.png#gh-light-mode-only)
![Le tableau des tâches à faire, où les tâches passent de À faire à En cours puis à Terminé](../images/task-dark.png#gh-dark-mode-only)

## 🪟 Vue divisée

Une seule barre d'onglets ne suffit pas toujours. Faites un clic droit sur un onglet de conversation pour diviser la vue **à droite** ou **vers le bas**, autant de fois que vous voulez : deux volets côte à côte, une pile de trois, une grille. Chaque groupe est un espace de travail à part entière — ses onglets, son en-tête, son propre bouton de nouvelle conversation — Claude Code peut donc refactoriser dans un volet pendant que Codex relit un diff dans le suivant.

Faites glisser un onglet d'un groupe à l'autre : sa session continue de streamer pendant le déménagement. Faites glisser la séparation entre deux groupes pour changer le partage de l'espace. Votre disposition est mémorisée par espace de travail, brouillons compris : rouvrez Dextra et la division revient, avec le texte jamais envoyé toujours dans le champ.

![Division de la zone de conversation en une grille de groupes d'onglets](../images/split-light.gif#gh-light-mode-only)
![Division de la zone de conversation en une grille de groupes d'onglets](../images/split-dark.gif#gh-dark-mode-only)

## 📄 Documents Office

Demandez une présentation, un rapport ou un classeur : l'agent produit un vrai `.pptx` / `.docx` / `.xlsx` — pendant que le volet de droite le rend en direct. Chaque modification arrive d'elle-même dans l'aperçu : les diapositives se remplissent, les tableaux prennent forme, les chiffres se posent dans les cellules. La diapositive 4 ne vous plaît pas ? Dites-le au message suivant — l'agent modifie le même fichier sur place et l'aperçu suit. Aucun export, aucune application Office externe, aucune sortie de Dextra.

![Un agent modifiant un document Office à côté de son aperçu en direct](../images/office-light.png#gh-light-mode-only)
![Un agent modifiant un document Office à côté de son aperçu en direct](../images/office-dark.png#gh-dark-mode-only)

## 💻 Espace de travail

Un seul espace de travail, tous les agents. Quel que soit celui qui travaille — Claude Code, Codex, Cursor —, il le fait dans le même éditeur, avec les mêmes diffs en direct et le même client git ; et ce qu'il produit, ce sont de vrais fichiers de votre dépôt, qui changent sous vos yeux. Rattachez d'autres répertoires — une bibliothèque partagée, un service voisin, le dépôt de documentation — et l'arborescence, la recherche et l'agent lui-même les traitent comme un seul espace de travail.

**Sessions.** Récupérez l'historique que vous avez déjà : les sessions passées de tous les agents installés, importées en un clic et reprenables là où vous les aviez laissées. Une fois dedans, elles cessent d'être des silos séparés — mentionnez une ancienne session avec `@` et l'agent auquel vous parlez peut la lire, même si un autre agent l'a écrite ; l'exécution Codex d'aujourd'hui repart donc de là où la session Claude Code de la semaine dernière s'est arrêtée. Aussi longue que devienne une conversation, elle s'ouvre sur ses derniers tours et charge le reste au fur et à mesure que vous remontez.

**Fichiers.** Les modifications de l'agent apparaissent sous forme de diffs à côté de la conversation, au fur et à mesure. Ouvrez n'importe quel fichier dans un vrai éditeur avec coloration syntaxique, envoyez un fichier — ou juste une sélection — directement à l'agent avec `⌘L`, et prévisualisez Markdown, HTML, images et documents Office dans le même volet.

**Git.** Un client complet, pas un simple indicateur d'état : committez directement depuis l'onglet Modifications — un message, Entrée — avec pull, fetch, push et remise à côté, et un historique qui montre quels commits sont poussés. Créez des branches, fusionnez, rebasez, réinitialisez ou comparez avec une autre branche, et mettez à jour ou poussez n'importe quelle branche sans basculer dessus. Les conflits ouvrent un éditeur de fusion à trois volets où vous acceptez bloc par bloc ou tapez vous-même la résolution. Et les worktrees réduisent le travail en parallèle à une seule action — une nouvelle branche, son propre répertoire et une conversation toute neuve enracinée dedans, pour qu'une flotte d'agents construise des fonctionnalités différentes en même temps sans se marcher sur les fichiers.

**Quand ça se passe mal.** Un tour qui échoue ne dit pas seulement que quelque chose a raté : sur Claude Code et Codex, il en nomme le type — problème de connexion, problème d'accès, limite atteinte, requête refusée, problème de service — et accroche sous le compositeur un bandeau ne portant que ce qui aiderait vraiment : Réessayer, Se connecter ou une nouvelle session. Les tentatives que l'agent fait de lui-même s'affichent en ambre et se résument en une seule ligne « Rétabli ». Et l'indicateur de connexion sous le compositeur est un bouton : cliquez pour connaître l'état réel de la session, avec une Reconnexion qui reprend au lieu de repartir de zéro.

## 📱 iPhone, iPad et Android

Les clients iOS et Android du projet d’origine se connectent au service web intégré de Dextra avec son URL et son jeton.

| iPhone et iPad | Android |
| :---: | :---: |
| <img src="../images/mobile-ios.jpg" alt="Démarrage d'une session depuis le client Dextra pour iOS" width="248" /> | <img src="../images/mobile-android.jpg" alt="La réponse d'un agent qui arrive en direct dans le client Dextra pour Android" width="248" /> |

## ✨ Points forts

- **[Agrégation des conversations](https://docs.codeg.app/guide/aggregation)** — importez les sessions de tous les agents supportés dans un espace de travail unifié et consultable, et reprenez-les là où vous vous étiez arrêté
- **[Collaboration multi-agents](https://docs.codeg.app/guide/multi-agent)** — mentionnez un agent avec `@` pour déléguer : les sous-agents de types différents s'exécutent chacun dans sa session, en parallèle, au sein d'une même tâche
- **[Tâches à faire](https://docs.codeg.app/guide/tasks)** — notez ce qu'il y a à faire et les agents vident la file, chaque tâche dans son propre worktree, pour n'atterrir sur votre branche qu'après votre relecture
- **[Agents personnalisés](https://docs.codeg.app/guide/custom-agents)** — enregistrez n'importe quel autre agent compatible ACP depuis le registre public ou son JSON de distribution ; Dextra l'installe, enregistre son historique et le traite comme un agent intégré
- **[L'espace de travail](https://docs.codeg.app/guide/workspace)** — toute la boucle d'ingénierie à côté de l'agent : arborescence, éditeur et diff, changements git, commit, terminal intégré et [plusieurs dossiers réunis en un seul espace de travail](https://docs.codeg.app/guide/workspace#work-across-several-folders)
- **[Vue divisée](https://docs.codeg.app/guide/workspace#split-the-conversation-view-into-groups)** — divisez la zone de conversation en autant de groupes d'onglets que vous voulez, faites glisser onglets et séparations entre eux, et retrouvez la disposition — brouillons compris — au redémarrage
- **[Git et worktrees](https://docs.codeg.app/guide/git)** — relisez et validez vos changements, gérez vos comptes Git distants et travaillez en parallèle grâce aux flux `git worktree` intégrés
- **[Utilisation des tokens](https://docs.codeg.app/guide/token-usage)** — derrière le compteur de la barre d'état, un rapport complet : tendances et taux de succès du cache, carte de chaleur d'activité, et répartitions par dossier, agent, modèle et session
- **[Canaux de discussion](https://docs.codeg.app/guide/chat-channels)** — pilotez vos agents depuis Telegram, Lark (Feishu) et WeChat : créez des tâches, approuvez des permissions, suivez l'avancement en direct
- **[Automatisations](https://docs.codeg.app/guide/automations)** — enregistrez un compositeur entièrement configuré comme une automatisation réutilisable, exécutée sans interface, selon un planning cron ou à la demande — en lançant une session, ou en déposant une tâche à faire que vous relirez plus tard
- **[Documents Office](https://docs.codeg.app/guide/office)** — créez, analysez, relisez et modifiez des `.docx` / `.xlsx` / `.pptx` via l'`officecli` intégré, avec aperçu en direct dans l'onglet
- **[Recherche scientifique](https://docs.codeg.app/guide/research)** — des compétences de recherche intégrées (formulation d'hypothèses, plan d'expérience, statistiques, visualisation, évaluation critique, recherche bibliographique) que n'importe quel agent peut invoquer
- **[Project Boot](https://docs.codeg.app/guide/project-boot)** — créez visuellement de nouveaux projets, avec aperçu en direct, puis ouvrez-les directement dans l'espace de travail
- **[MCP](https://docs.codeg.app/guide/mcp) & [Skills](https://docs.codeg.app/guide/skills)** — scan des serveurs locaux, recherche et installation depuis le registre, et compétences gérées au niveau global ou projet
- **[À votre image](https://docs.codeg.app/reference/settings/appearance)** — recolorez n'importe lequel des douze thèmes token par token, réglez l'arrondi des angles pour toute l'app, importez et exportez des thèmes en JSON shadcn, ou écrivez votre propre CSS
- **Service web de Dextra** — les navigateurs autorisés peuvent se connecter au service intégré de l’application.
- **[iPhone, iPad et Android](https://docs.codeg.app/getting-started/installation#mobile-apps)** — des clients mobiles natifs reliés à votre poste ou à votre serveur : lancez des sessions, recevez les réponses en flux, approuvez les permissions et parcourez vos projets où que vous soyez

## 📦 Installation et exécution

**Bureau** — téléchargez les installateurs dans l’aide de Convene après leur publication. Le [dépôt Dextra](https://hm.ziqi.ac.cn:9400/zzq/dextra) contient le code du client.

**Mobile** — utilisez le [client iOS](https://github.com/xintaofei/codeg-ios) ou [client Android](https://github.com/xintaofei/codeg-android) du projet d’origine et saisissez l’URL et le jeton du service web de Dextra.

## 🔒 Confidentialité et sécurité

- Local d'abord par défaut pour l'analyse, le stockage et les opérations sur les projets — les accès réseau n'ont lieu que sur des actions que vous déclenchez
- Les modes web et serveur sont protégés par une authentification par jeton
- Prise en charge du proxy système pour les environnements d'entreprise

Détails dans [Confidentialité et sécurité](https://docs.codeg.app/reference/privacy).

## 👥 Communauté

Le code et les mises à jour se trouvent dans le [dépôt Dextra](https://hm.ziqi.ac.cn:9400/zzq/dextra).

## 🙏 Remerciements

- [Agent Client Protocol](https://agentclientprotocol.com) — le socle qui permet à Dextra de se connecter à tous les agents qu'il supporte
- [Superpowers](https://github.com/obra/superpowers) — alimente le module de compétences d'experts de Dextra
- [OfficeCLI](https://github.com/iOfficeAI/OfficeCLI) — alimente le flux de travail des documents Office de Dextra
- [scientific-agent-skills](https://github.com/K-Dense-AI/scientific-agent-skills) — alimente les compétences de Recherche scientifique de Dextra (sous-ensemble sous licence MIT)

## 📜 Licence

Apache-2.0. Voir [LICENSE](../../LICENSE).
