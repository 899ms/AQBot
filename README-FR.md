[简体中文](./README.md) | [繁體中文](./README-ZH-TW.md) | [English](./README-EN.md) | [日本語](./README-JA.md) | [한국어](./README-KO.md) | **Français** | [Deutsch](./README-DE.md) | [Español](./README-ES.md) | [Русский](./README-RU.md) | [हिन्दी](./README-HI.md) | [العربية](./README-AR.md)

[![AQBot](https://socialify.git.ci/AQBot-Desktop/AQBot/image?description=1&font=JetBrains+Mono&forks=1&issues=1&logo=https%3A%2F%2Fgithub.com%2FAQBot-Desktop%2FAQBot%2Fblob%2Fmain%2Fsrc%2Fassets%2Fimage%2Flogo.png%3Fraw%3Dtrue&name=1&owner=1&pattern=Floating+Cogs&pulls=1&stargazers=1&theme=Auto)](https://github.com/AQBot-Desktop/AQBot)

AQBot est un espace de travail IA local pour ordinateur qui réunit chat multi-fournisseurs, agents ACP, bases de connaissances, outils MCP et passerelle API, tout en vous laissant le contrôle des données et fichiers stockés sur votre machine.

## Captures d'écran

| Rendu des graphiques de chat | Fournisseurs et modèles |
|:---:|:---:|
| ![](.github/images/s1-0412.png) | ![](.github/images/s2-0412.png) |

| Base de connaissances | Mémoire |
|:---:|:---:|
| ![](.github/images/s3-0412.png) | ![](.github/images/s4-0412.png) |

| Agent - Demande | Passerelle API en un clic |
|:---:|:---:|
| ![](.github/images/s5-0412.png) | ![](.github/images/s6-0412.png) |

| Sélection du modèle de chat | Navigation des chats |
|:---:|:---:|
| ![](.github/images/s7-0412.png) | ![](.github/images/s8-0412.png) |

| Agent - Approbation des permissions | Aperçu de la passerelle API |
|:---:|:---:|
| ![](.github/images/s9-0412.png) | ![](.github/images/s10-0412.png) |

## Fonctionnalités

### Chat et modèles

- **Chat multi-fournisseurs** — Connectez OpenAI, Claude, Gemini, DeepSeek, Qwen et tout endpoint compatible OpenAI avec Base URL, API Path, headers et proxy.
- **Onboarding fournisseur** — Utilisez les liens aqbot:// et l’import CC Switch pour importer des profils fournisseur après confirmation.
- **Gestion des modèles** — Synchronisez les modèles distants, groupes, latence, capacités, contexte, sampling, profils de raisonnement et extra_body par modèle.
- **Workflows de conversation** — Streaming, blocs de réflexion, versions de messages, branches, état de génération du titre, compression et réponses multi-modèles.

### AI Agent

- **Deux façons d'utiliser les agents** — AQBot propose à la fois un agent intégré au chat et un atelier ACP indépendant. Le premier utilise les API de fournisseurs que vous avez configurées ; le second se connecte à des processus d'agents externes compatibles ACP, afin de choisir selon le modèle et le workflow.
- **Agent de chat (API fournisseur)** — Passez une conversation ordinaire en mode Agent et utilisez directement l'API du fournisseur et du modèle configurés pour lire ou modifier des fichiers, exécuter des commandes et analyser du code dans un répertoire isolé.
- **Contrôle de l'agent de chat** — Choisissez entre confirmation systématique, acceptation des modifications ou accès complet, examinez les appels d'outils et les autorisations en temps réel, et suivez les tokens et le coût de chaque exécution.
- **Atelier d'agents ACP** — Exécutez des agents de programmation compatibles via l'[Agent Client Protocol (ACP)](https://agentclientprotocol.com/) dans un atelier dédié, avec réponses, raisonnement et appels d'outils en streaming.
- **ACP Registry et intégrations personnalisées** — Ajoutez depuis le Registry des agents tels que Codex, Claude Agent, Gemini CLI, Cline, OpenCode et Grok Build, ou configurez commande, arguments, variables d'environnement et icône ; chaque agent peut être activé et réordonné librement.
- **Projets ACP et discussions directes** — Classez les fils par projet, épinglez les sessions importantes et restaurez automatiquement votre dernier espace de travail, ou démarrez sans projet dans un répertoire de travail isolé.
- **Interactions de session ACP complètes** — Changez les modèles, modes et options exposés par chaque agent et utilisez pièces jointes, questionnaires, validation de plans, suivi de progression et restauration de l'état après rechargement ; les demandes d'outils peuvent être autorisées une fois ou toujours pour la session en cours.

### Rôles

- **Gestion locale des rôles** — Enregistrez system prompts, avatars, tags, messages d’ouverture, questions de départ, température et Top P comme modèles de conversation réutilisables.
- **Utilisation en un clic** — Créez par défaut une nouvelle conversation de rôle, ou appliquez le rôle à la conversation courante depuis le menu ; les conversations de rôle gardent le nom, l’avatar et le badge bleu Rôles.
- **Marketplace** — Recherchez et installez des rôles depuis prompts.chat et PlexPt 中文, puis utilisez-les localement.

### Gestion des skills

- **Répertoires multi-sources** — Gérez les racines AQBot, Codex, Claude et Agents, dont `~/.aqbot/skills`, `~/.codex/skills`, `~/.claude/skills` et `~/.agents/skills`.
- **Mes skills** — Filtrez par source, activez/désactivez, consultez les détails, copiez le nom, ouvrez le dossier et désinstallez.
- **Groupes et cibles d'installation** — Repliez les skills par group, activez/désactivez en lot, ouvrez le dossier du groupe, désinstallez le groupe entier et installez depuis `owner/repo` ou une URL GitHub vers la cible choisie.
- **Marketplace** — Recherchez dans skills.sh et GitHub, prévisualisez les détails, ouvrez GitHub et voyez l'état d'installation.

### Rendu de contenu

- **Markdown et maths** — Rendu Markdown, code, tableaux, tâches et LaTeX dans les conversations streamées.
- **Code, diagrammes et artifacts** — Monaco, Mermaid, D2 et panneau Artifact pour code, notes Markdown, rapports et aperçus.
- **Fragments HTML** — Prévisualisez les fragments HTML générés avec les correctifs récents de stabilité du streaming.

### Recherche et connaissances

- **Recherche Web** — Tavily, Exa, Zhipu WebSearch, Bocha avec sources citées et génération de requêtes.
- **Bases de connaissances locales** — Indexez vos documents avec sqlite-vec, réglez retrieval/rerank et inspectez les retours de récupération.
- **Gestion du contexte** — Ajoutez fichiers, résultats de recherche, extraits, mémoires et sorties d’outils au contexte.

### Outils et extensions

- **Protocole MCP** — Exécutez des serveurs Model Context Protocol en stdio, SSE ou StreamableHTTP.
- **Outils intégrés** — Utilisez @aqbot/fetch et la recherche de fichiers sans serveur séparé.
- **Limite de boucle outils** — Configurez le nombre maximal de boucles MCP et récupérez mieux les sessions bloquées.

### Passerelle API

- **Passerelle locale** — Exposez OpenAI Chat Completions, OpenAI Responses, Claude natif et Gemini natif depuis l’app.
- **Accès et observabilité** — Gérez clés, SSL/TLS, logs de requêtes et statistiques localement.
- **Templates clients** — Templates pour Claude Code, Codex CLI, OpenCode, Gemini CLI et clients personnalisés.

### Import et sauvegarde

- **Imports tiers** — Importez ChatGPT, Cherry Studio et Kelivo avec aperçu, avertissements et gestion des doublons.
- **Migration fournisseurs/fichiers** — Les imports Cherry Studio/Kelivo peuvent migrer fournisseurs, clés API et pièces jointes.
- **Sauvegardes** — Sauvegarde/restauration via dossiers locaux, WebDAV ou stockage compatible S3.

### Bureau et sécurité

- **Chiffrement local** — État dans ~/.aqbot/, fichiers utilisateur dans ~/Documents/aqbot/, clés API protégées par AES-256.
- **Intégration desktop** — Tray, always-on-top, raccourcis globaux, auto-start, proxy et vérification des mises à jour.
- **11 langues** — Interface disponible en chinois simplifié/traditionnel, anglais, japonais, coréen, français, allemand, espagnol, russe, hindi et arabe.

## Plateformes prises en charge

| Plateforme | Architecture |
|------------|-------------|
| macOS | Apple Silicon (arm64), Intel (x86_64) |
| Windows 10/11 | x86_64, arm64 |
| Linux | x86_64 (AppImage/deb/rpm), arm64 (AppImage/deb/rpm) |

## Démarrage rapide

Rendez-vous sur la page [Releases](https://github.com/AQBot-Desktop/AQBot/releases) et téléchargez le programme d'installation pour votre plateforme.

## FAQ

### macOS : « L'application est endommagée » ou « Impossible de vérifier le développeur »

Comme l'application n'est pas signée par Apple, macOS peut afficher l'une des invites suivantes :

- « AQBot » est endommagé et ne peut pas être ouvert
- « AQBot » ne peut pas être ouvert car Apple ne peut pas vérifier l'absence de logiciels malveillants

**Étapes pour résoudre le problème :**

**1. Autoriser les applications de « N'importe où »**

```bash
sudo spctl --master-disable
```

Ensuite, allez dans **Réglages Système → Confidentialité et sécurité → Sécurité** et sélectionnez **N'importe où**.

**2. Supprimer l'attribut de quarantaine**

```bash
sudo xattr -dr com.apple.quarantine /Applications/AQBot.app
```

> Astuce : Vous pouvez faire glisser l'icône de l'application dans le terminal après avoir tapé `sudo xattr -dr com.apple.quarantine `.

**3. Étape supplémentaire pour macOS Ventura et versions ultérieures**

Après avoir effectué les étapes ci-dessus, le premier lancement peut encore être bloqué. Allez dans **Réglages Système → Confidentialité et sécurité**, puis cliquez sur **Ouvrir quand même** dans la section Sécurité. Cette opération n'est nécessaire qu'une seule fois.

## Communauté
- [LinuxDO](https://linux.do)

## Licence

Ce projet est sous licence [AGPL-3.0](LICENSE).
