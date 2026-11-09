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
- Backend : `src-tauri/src/lib.rs` (lecture : `get_registry_paths`) et `src-tauri/src/actions.rs` (écriture : `create_registry_backup`, `restore_registry_backup`, `list_backups`, `preview_cleanup`, `remove_orphans`, `deduplicate_paths`, `is_admin`). Crate `devenv_doctor_lib`, dépendances `winreg` 0.56 et `chrono`. Sauvegardes dans `%APPDATA%m.txfai.devenvdoctorbackups`.

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

### Écriture Registre : KEY_SET_VALUE, droits admin, garde-fous
- **Problème / Erreur :** écrire dans HKLM sans élévation échoue (`ERROR_ACCESS_DENIED`, code 5 / `PermissionDenied`) ; `open_subkey` (KEY_READ) réussit toujours, donc ne prouve rien sur le droit d'écrire.
- **Solution retenue :** `open_subkey_with_flags(key, KEY_SET_VALUE)` : s'il réussit, on peut écrire (`can_write`, utilisé aussi par `is_admin`). Écriture via `set_raw_value("Path", &RegValue { bytes: Cow::Owned(encode_reg_string(..)), vtype })` en reprenant le type d'origine. Erreur de droits HKLM renvoyée avec le préfixe stable `AdminRequired: ...` (l'UI teste `startsWith("AdminRequired")`).
- **Règle à appliquer :** vérifier les droits de TOUS les scopes à modifier AVANT la 1re écriture (pas de modification partielle) ; sauvegarde JSON automatique avant toute écriture ; refuser un PATH vide ou un type autre que REG_SZ/REG_EXPAND_SZ ; ne jamais écrire en `REG_SZ` ce qui était `REG_EXPAND_SZ` (les `%VAR%` cesseraient d'être développées). Ne jamais écrire dans le vrai Registre depuis un test : clé jetable `HKCU\Software\DevEnvDoctorTest-<pid>` supprimée à la fin. La validation « UI » n'écrit pas non plus : comparer l'empreinte des deux Path avant/après.

### Orphelins : ce qu'on n'ose pas supprimer
- **Problème / Erreur :** « dossier introuvable » ne prouve pas l'inutilité : variable `%X%` non résolue dans ce processus, chemin réseau `\\srv\...` injoignable, lecteur amovible absent.
- **Solution retenue :** `is_safe_orphan` : n'est ciblé que ce dont l'expansion ne contient plus `%` et n'est pas un chemin UNC ; le reste est compté dans `skipped_unsafe` et signalé dans la modale.
- **Règle à appliquer :** rester prudent sur la suppression ; le dédoublonnage compare Système puis Utilisateur (1re occurrence gardée), les cibles sont identifiées par INDEX (deux entrées au texte identique ne doivent pas partir ensemble).

### Sauvegardes : où, et comment les relire
- **Problème / Erreur :** un dossier `backups/` relatif écrit dans `src-tauri/` en dev, ce que `tauri dev` surveille (rebuild en boucle) ; un nom de fichier venant de l'UI peut sortir du dossier (`..\..`).
- **Solution retenue :** dossier `%APPDATA%\com.txfai.devenvdoctor\backups\` (`app.path().app_data_dir()`, via `tauri::Manager`) ; la restauration ne prend que le NOM de fichier (`.json`, `file_name()`), valide la version et les types, et sauvegarde l'état courant avant d'écrire.
- **Règle à appliquer :** jamais de chemin d'UI suivi tel quel ; arguments Tauri : `backup_file` côté Rust = `backupFile` côté JS ; commandes lourdes en `async`.

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
