# CLAUDE.md - Mémoire & Directives du Projet DevEnv Doctor

## Vue d'ensemble & Architecture
- **Projet :** DevEnv Doctor (Nettoyeur de %PATH% Windows, Profils Dev & Backup GitHub).
- **Stack cible :** Rust (Backend) + Tauri v2 + Web Dark Mode / Cyberpunk (Frontend).
- **Périmètre :** Manipulation sécurisée du Registre Windows (`HKCU\Environment` et `HKLM\SYSTEM\...`).

## Commandes clés (À compléter au fil de l'initialisation)
Gestionnaire de paquets : **npm** (pnpm non installé). Frontend : Vite 7 + Vanilla JS + Tailwind 4 (`@tailwindcss/vite`, Dark Mode), port fixe 1420.
- Installer : `npm install`
- Dev (fenêtre Tauri + Vite) : `npm run tauri dev` (1re compilation Rust longue)
- Build frontend seul : `npm run build` → `dist/`
- Build de l'app : `npm run tauri build`
- Tests Rust : `cd src-tauri && cargo test`
- Lint Rust : `cd src-tauri && cargo clippy` (non encore lancé) ; format : `cargo fmt`
- Rust installé via winget `Rustlang.Rustup` (stable MSVC). Dans un shell déjà ouvert, `cargo` n'est pas dans le PATH : `$env:Path += ";$env:USERPROFILE\.cargo\bin"`.
- Backend : `src-tauri/src/lib.rs`. Crate : `devenv_doctor_lib`, dépendance `winreg` 0.56. Commande `get_registry_paths` (lecture seule) → `RegistryPathReport { user, system }` ; logique pure et testée : `expand_env`, `decode_reg_string`, `analyze_registry_path` (existence et lecture réelle injectées).

## Conventions de Code & Git
- Commits conventionnels (`feat:`, `fix:`, `docs:`, `refactor:`).
- Auteur Git : `TxF-AI <206823544+TxF-AI@users.noreply.github.com>`.

## Registre des Erreurs & Leçons Apprises (Memory Log)
*Mise à jour obligatoire : À chaque commande échouée, bug complexe (> 2 essais) ou comportement inattendu Windows/Rust/Tauri, consigner ici le problème, la solution et la règle stricte à suivre pour éviter de consommer des tokens inutilement.*

*Avant toute tâche lourde ou code système : relire ce fichier et appliquer les leçons ci-dessous. Mettre à jour cette section avant de clôturer la tâche.*

Format d'une entrée :
- **Problème / Erreur :** ...
- **Solution retenue :** commande ou correctif exact qui a fonctionné.
- **Règle à appliquer :** consigne stricte pour l'avenir.

### Rust absent après installation winget
- **Problème / Erreur :** `cargo`/`rustc` introuvables ; après `winget install Rustlang.Rustup`, le shell en cours ne voit toujours pas `~\.cargo\bin`.
- **Solution retenue :** `$env:Path += ";$env:USERPROFILE\.cargo\bin"` puis `rustup default stable-x86_64-pc-windows-msvc` (les Build Tools VS 2022 C++ étaient déjà là).
- **Règle à appliquer :** vérifier `cargo --version` avant toute tâche Rust ; ne jamais lancer `rustup` sans avoir ajouté `.cargo\bin` au PATH de la session.

### Le PATH lu par l'app n'est pas le PATH de l'utilisateur
- **Problème / Erreur :** en `tauri dev`, `std::env::var("PATH")` contient des entrées ajoutées par cargo/npm (`target\debug\deps`, `node_modules\.bin` de chaque dossier parent, rustlib) : orphelins et entrées parasites dans la liste.
- **Solution retenue :** aucune pour l'instant (constat ; l'affichage de démo reflète le PATH du *processus*).
- **Règle à appliquer :** pour une vraie analyse, lire le Registre (`HKCU\Environment\Path` et `HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment\Path`, valeurs `REG_EXPAND_SZ` à développer) et non l'environnement du processus. Ne jamais déduire un état système du PATH d'un process lancé par un outil de dev.

### Registre : REG_SZ vs REG_EXPAND_SZ, types winreg
- **Problème / Erreur :** une valeur `Path` peut être `REG_SZ` (les `%VAR%` ne sont PAS développées par Windows) ou `REG_EXPAND_SZ` (développées). Développer un REG_SZ afficherait un chemin que Windows n'utilise pas.
- **Solution retenue :** `key.get_raw_value("Path")` pour avoir `vtype` ; développement seulement si `vtype == REG_EXPAND_SZ` ; décodage UTF-16 LE et retrait du NUL final (`decode_reg_string`). Les deux Path de cette machine sont en REG_EXPAND_SZ.
- **Règle à appliquer :** ne jamais utiliser `get_value::<String>` (perd le type). Une variable `%X%` inconnue reste littérale (comme `ExpandEnvironmentStrings`). Test d'existence sur le chemin développé, sans guillemets. Ordre Windows : Système puis Utilisateur ; les doublons sont comptés entre scopes, la 1re occurrence (Système) gagne. Écriture future dans HKLM : droits admin requis ; garder REG_EXPAND_SZ à l'écriture pour ne pas casser les `%VAR%`.

### winreg 0.56 : type des clés racine
- **Problème / Erreur :** `error[E0308]: mismatched types` en passant `HKEY_LOCAL_MACHINE` à une fonction typée `isize`.
- **Solution retenue :** le type est `winreg::HKEY` (`use winreg::{RegKey, HKEY};`).
- **Règle à appliquer :** utiliser `winreg::HKEY` pour tout paramètre de racine, pas `isize`.

### Capture et clic sur la fenêtre Tauri pour valider l'UI
- **Problème / Erreur :** `SetCursorPos` + `mouse_event` n'ont pas cliqué (le curseur n'a pas bougé) ; impossible de tester un onglet par clic.
- **Solution retenue :** capture via `PrintWindow` (user32) de la fenêtre `devenv-doctor` ; pour voir un autre onglet, changer temporairement l'état initial dans `src/main.js` (HMR Vite recharge), capturer, puis remettre.
- **Règle à appliquer :** ne pas insister sur l'injection de souris ; arrêter seulement ses propres processus (PID du `cmd` lancé + `Get-Process devenv-doctor`), jamais « tuer tous les node/cargo ».

### Template create-tauri-app « vanilla » sans bundler
- **Problème / Erreur :** le template vanilla sert `src/` brut (pas de Vite), donc pas de Tailwind.
- **Solution retenue :** `index.html` à la racine + `src/main.js`, Vite + `@tailwindcss/vite`, `tauri.conf.json` avec `devUrl` `http://localhost:1420`, `beforeDevCommand` `npm run dev`, `frontendDist` `../dist`.
- **Règle à appliquer :** avec `withGlobalTauri: true`, l'API est `window.__TAURI__.core.invoke` (pas besoin du paquet `@tauri-apps/api`). Garder le plugin `opener` hors du projet tant qu'il est inutile (retirer aussi `opener:default` des capabilities).

### Outil Write sur fichier copié
- **Problème / Erreur :** `Write` refuse d'écraser un fichier qui n'a pas été lu dans la session.
- **Règle à appliquer :** après une copie de scaffolding, `Read` chaque fichier à remplacer avant `Write`/`Edit`.
