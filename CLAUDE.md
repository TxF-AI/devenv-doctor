# CLAUDE.md - Mémoire & Directives du Projet DevEnv Doctor

## Vue d'ensemble & Architecture
- **Projet :** DevEnv Doctor (Nettoyeur de %PATH% Windows, Profils Dev & Backup GitHub).
- **Stack cible :** Rust (Backend) + Tauri v2 + Web Dark Mode / Cyberpunk (Frontend).
- **Périmètre :** Manipulation sécurisée du Registre Windows (`HKCU\Environment` et `HKLM\SYSTEM\...`).
- **Version courante :** v0.3.0 (dépôt public https://github.com/TxF-AI/devenv-doctor). La version affichée par l'app vient de `src-tauri/tauri.conf.json` : la garder égale à `Cargo.toml` et `package.json`.

### Fonctionnalités (v0.2.0)
- **Analyse (lecture seule) :** PATH Utilisateur (HKCU) et Système (HKLM) lus dans le Registre, `%VAR%` développées selon REG_SZ / REG_EXPAND_SZ ; statuts Valide / Orphelin / Doublon ; tableau dense avec recherche, filtres état et scope (Ctrl+F) ; longueur de chaque PATH avec risque (seuils 1024 / 2048 / 8192) ; conflits de binaires de même nom.
- **Nettoyage (écriture) :** suppression des orphelins sûrs, dédoublonnage, compression (`%LOCALAPPDATA%`, `%APPDATA%`, `%USERPROFILE%`, `%windir%`, seulement si plus court), réordonnancement ⬆⬇ dans un scope. Toujours : aperçu, modale « diff », sauvegarde automatique, droits vérifiés avant d'écrire (`AdminRequired` pour HKLM), puis `WM_SETTINGCHANGE`.
- **Sauvegardes :** JSON horodaté dans `%APPDATA%\com.txfai.devenvdoctor\backups\`, restauration en un clic avec aperçu en diff (variables touchées comprises).
- **Profils & Dotfiles :** export/import JSON (sans secrets ni nom d'utilisateur), presets `Minimal Clean` / `WebDev` / `GameDev`, application limitée à HKCU ; export `setup-env.ps1` (PC vierge, `-WhatIf`, sans droits admin).
- **Langues (FR/EN) :** sélecteur `#lang` dans la barre du haut ; `src/i18n.js` (dictionnaires, `t()`, langue du système par défaut, mémorisée dans localStorage, attributs `data-i18n`/`-html`/`-ph`/`-title`/`-aria`) ; commande `set_language` (backend : `src-tauri/src/i18n.rs`, macro `tr!(fr, en)`). Tout nouveau texte visible = une clé dans les deux langues ; tout message d'erreur Rust = `tr!`. Le préfixe `AdminRequired:` ne change jamais. Les raisons de sauvegarde (`manuelle`, `avant-compression`…) sont des identifiants stockés dans les fichiers : traduits à l'affichage seulement (`reasonLabel`). Les textes venant du backend (risques, presets, raisons de nettoyage) sont redemandés au changement de langue ; le script `setup-env.ps1` suit la langue au moment de l'export (`PS_TEMPLATE` / `PS_TEMPLATE_EN`). Les tests qui lisent un message prennent `crate::i18n::LANG_LOCK`.
- **Interface :** thème « HUD / dev tool » (obsidienne, accents réservés aux statuts), icônes SVG inline, LED `WM_SETTINGCHANGE` dans le pied de page.

## Commandes clés
Gestionnaire de paquets : **npm** (pnpm non installé). Frontend : Vite 7 + Vanilla JS + Tailwind 4 (`@tailwindcss/vite`, Dark Mode), port fixe 1420.
- Installer : `npm install`
- Dev (fenêtre Tauri + Vite) : `npm run tauri dev` (1re compilation Rust longue)
- Build frontend seul : `npm run build` → `dist/`
- Build de l'exécutable final : `npm run tauri build -- --no-bundle` → `src-tauri\target\release\devenv-doctor.exe` (sans installateur). Sans `--no-bundle`, `targets: "all"` télécharge WiX/NSIS pour produire MSI/setup : à ne lancer qu'en connaissance de cause. Build release long (LTO).
- Tests Rust : `cd src-tauri && cargo test`
- Lint Rust : `cd src-tauri && cargo clippy --all-targets` (propre : 0 avertissement) ; format : `cargo fmt`
- Rust installé via winget `Rustlang.Rustup` (stable MSVC). Dans un shell déjà ouvert, `cargo` n'est pas dans le PATH : `$env:Path += ";$env:USERPROFILE\.cargo\bin"`.
- Backend : `src-tauri/src/lib.rs` (lecture : `get_registry_paths`), `src-tauri/src/actions.rs` (écriture : `create_registry_backup`, `restore_registry_backup`, `preview_restore`, `list_backups`, `preview_cleanup`, `remove_orphans`, `deduplicate_paths`, `is_admin`, diffusion `WM_SETTINGCHANGE`), `src-tauri/src/profiles.rs` (`list_presets`, `export_profile`, `import_profile`, `preview_profile`, `apply_profile`, `export_powershell`) et `src-tauri/src/optimize.rs` (`get_path_lengths`, `preview_compress`, `compress_path`, `move_path_entry`, `analyze_conflicts`). Crate `devenv_doctor_lib` ; dépendances `winreg` 0.56, `chrono`, `windows-sys` (Win32_Foundation, Win32_UI_WindowsAndMessaging). Sauvegardes : `%APPDATA%\com.txfai.devenvdoctor\backups\` ; exports de profils : `%APPDATA%\com.txfai.devenvdoctor\profiles\<nom>.json` (JSON uniquement, pas de YAML).

## Conventions de Code & Git
- Commits conventionnels (`feat:`, `fix:`, `docs:`, `refactor:`).
- Auteur Git : `TxF-AI <206823544+TxF-AI@users.noreply.github.com>`.
- Publication : `gh` (authentifié en `TxF-AI`) ; dépôt public `TxF-AI/devenv-doctor`, branche `main`. Ordre d'une release : mettre à jour la version (3 fichiers) → `cargo test` + `cargo clippy --all-targets` → commit → build release → tag annoté `vX.Y.Z` → `git push origin main` puis `git push origin <tag>` → `gh release create vX.Y.Z <exe> --verify-tag --notes-file <fichier hors dépôt ou supprimé après>` avec le SHA-256 dans les notes → mettre à jour `README.md` (titre, badges, taille, lien direct, SHA-256). Aucune donnée personnelle dans les fichiers suivis (vérifier avec `git grep -i -E "$env:USERNAME|gmail"`).

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
- **Solution retenue :** résolu depuis v0.1.x : `get_registry_paths` lit le Registre (`read_path_value`), plus jamais l'environnement du processus.
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

### WM_SETTINGCHANGE : prévenir Windows après une écriture
- **Problème / Erreur :** modifier le Registre ne suffit pas : l'explorateur et les nouveaux terminaux gardent l'ancien environnement jusqu'au redémarrage de session.
- **Solution retenue :** `SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, 0, "Environment" UTF-16 terminé par NUL, SMTO_ABORTIFHUNG, 3000, &mut res)` (`windows-sys` 0.61, features `Win32_Foundation` + `Win32_UI_WindowsAndMessaging`), appelé seulement APRÈS une écriture réussie. Retour 0 = délai dépassé/échec : ce n'est PAS une erreur d'écriture, l'UI affiche un avertissement et demande de relancer les terminaux. Les processus déjà ouverts gardent leur copie de l'environnement dans tous les cas.
- **Règle à appliquer :** jamais de diffusion si rien n'a été écrit ; le `LPARAM` doit pointer une chaîne qui vit pendant tout l'appel (appel synchrone) ; garder le bloc `unsafe` minimal et commenté.

### Profils : un fichier importé est une entrée non fiable
- **Problème / Erreur :** un profil JSON venu d'ailleurs pourrait réécrire `COMSPEC`/`PATHEXT`/`Path`, ou ajouter un dossier piégé ; un export naïf fuit tokens et nom d'utilisateur dans un dépôt dotfiles public.
- **Solution retenue :** `validate_profile` (version, tailles, noms `[A-Za-z_][A-Za-z0-9_]*`, pas de `;`/NUL) ; liste `PROTECTED_VARS` ignorée à l'application ; seul HKCU est modifié (`path_system` informatif) ; aperçu + modale + sauvegarde auto, le plan est RECALCULÉ dans `apply_profile`. Export : noms contenant TOKEN/SECRET/PASS/KEY/CRED/AUTH/PRIVATE/SESSION/COOKIE exclus et listés ; `%USERPROFILE%` remplace le dossier de profil (insensible à la casse).
- **Règle à appliquer :** ne jamais appliquer un aperçu périmé ; ne jamais faire confiance à un champ du fichier ; un « Replace » vide n'efface jamais le PATH (`path_user` vide = inchangé, résultat vide refusé). Les noms de variables non secrets peuvent rester personnels (ex. identifiants) : relire l'export avant de le publier.

### Restauration des variables touchées par un profil
- **Problème / Erreur :** la sauvegarde ne couvrait que `Path`, donc un profil qui ajoute des variables n'était pas annulable.
- **Solution retenue :** champ `touched_variables` (`#[serde(default)]`, anciennes sauvegardes toujours lisibles) : ancienne valeur, ou `null` = n'existait pas → supprimée à la restauration. Seules les variables touchées sont revertées (pas de resynchronisation totale qui effacerait d'autres variables).
- **Règle à appliquer :** toute nouvelle écriture hors `Path` doit enregistrer l'état précédent dans `touched_variables` AVANT d'écrire. Validation sans toucher à tes vrais réglages : profil jetable `DEVENV_DOCTOR_SELFTEST`, appliqué puis restauré, empreinte du Registre comparée avant/après.

### Interface : charte, structure et pièges du design (refonte « HUD / dev tool »)
- **Charte :** fond `#090a0f`, surfaces `#111319`, bordures `#1e2230` ; accents `--ok #00ff9d`, `--info #00e5ff`, `--bad #ff3366`, `--warn #ffb020` réservés aux statuts ; police `JetBrains Mono → Fira Code → Cascadia Mono → ui-monospace` (aucune police chargée en ligne : l'app n'envoie rien sur Internet). Variables CSS dans `src/styles.css`, composants en classes (`.btn`, `.chip`, `.badge`, `.dl`…), pas de longues chaînes d'utilitaires Tailwind.
- **Structure :** `index.html` (squelette, `data-icon` hydratés par `src/icons.js`, icônes Lucide inline) ; `src/main.js` (logique). Vue PATH = tableau dense à colonnes fixes (`table-layout: fixed` + `colgroup`, sinon les colonnes sautent au filtrage), filtres texte/état/scope (Ctrl+F), pied de page avec LED `WM_SETTINGCHANGE`. Modale = `<dialog>` + `::backdrop { backdrop-filter: blur(12px) }` (= `backdrop-blur-md`), contenu en diff (`@@` hunk, `−`/`+`, lignes inchangées repliées).
- **Pièges :** (1) `scrollbar-width`/`scrollbar-color` standard suffisent dans WebView2 ; ne pas mélanger avec `::-webkit-scrollbar` (ignoré). (2) Chaque diff doit venir du backend (`preview_cleanup`, `preview_restore`, `preview_profile`) : jamais reconstruit à partir d'un état périmé de l'UI. (3) Un fichier édité par un hook/HMR externe doit être relu (`Read`) avant `Write` sinon l'écriture est refusée. (4) Validation visuelle : un crochet TEMPORAIRE en fin de `main.js` (marqué `TEMP-VALIDATION`) qui parcourt les écrans, puis le retirer (`grep -c TEMP-VALIDATION` doit valoir 0 avant commit) ; un rechargement HMR relance le crochet.

### Compression du PATH : la « compression » peut rallonger
- **Problème / Erreur :** `C:\Windows` (10 car.) → `%SystemRoot%` (12) ou `C:\Users\Moi` (12) → `%USERPROFILE%` (13) AUGMENTE la taille ; et `%USERPROFILE%` dans le PATH Système changerait de sens selon la session.
- **Solution retenue :** `optimize.rs` : `substitutions()` du préfixe le plus long au plus court (`%LOCALAPPDATA%`, `%APPDATA%`, `%USERPROFILE%` réservés au scope Utilisateur ; puis `%SystemRoot%`, `%windir%` pour Windows) ; `compress_entry` n'applique un remplacement que si l'entrée devient STRICTEMENT plus courte (donc `%windir%`, pas `%SystemRoot%`) ; préfixe de dossier ENTIER seulement (`C:\Users\MoiEtAutre` intact) ; entrées déjà variabilisées ou entre guillemets intactes. Ajout de `%LOCALAPPDATA%`/`%APPDATA%` (hors cahier des charges initial) : c'est là que se trouve le gain réel (−177 car. sur ce PC). Une valeur REG_SZ qui reçoit un `%VAR%` passe en REG_EXPAND_SZ (`commit_scope_edits`).
- **Règle à appliquer :** mesurer en UTF-16 (`encode_utf16().count()`), jamais en octets ; seuils `setx` 1024 / 2048 / 8192 dans `assess` ; ne jamais promettre un gain sans le calculer ; tester chaque règle avec un profil « court » ET « long ».

### Réordonnancement : garde-fous et normalisation
- **Problème / Erreur :** un index vu par l'UI peut être périmé ; une série de flèches créerait une sauvegarde par clic ; réécrire le PATH retire un `;` final (diff d'octets alors que les entrées sont identiques).
- **Solution retenue :** `move_path_entry(scope, index, direction, expected_path)` refuse si le texte à cet index a changé ; sauvegarde réutilisée si la plus récente a la même raison (`avant-reordonnancement`) et moins de 5 min (`can_reuse_backup`) ; Système avant Utilisateur : on ne réordonne QUE dans un scope ; HKLM sans admin → `AdminRequired` avant toute écriture. Après un essai réel, remettre la valeur exacte depuis la sauvegarde (`[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment',$true).SetValue('Path',$raw,'ExpandString')`) et comparer le hash.
- **Règle à appliquer :** tout test qui écrit pour de vrai doit être son propre inverse et terminer par une comparaison d'empreinte ; signaler honnêtement une normalisation (`;` final, segments vides) au lieu de dire « identique ».

### Conflits de binaires
- **Problème / Erreur :** comparer les noms de fichiers de tous les dossiers du PATH produit beaucoup de bruit (`setup.exe`, `unins000.exe`, `update.exe`…) et peut être lent (UNC).
- **Solution retenue :** `find_conflicts` pure (testée avec un faux `list_dir`) ; seuls `.exe/.cmd/.bat/.com` ; `is_noise` écarte `unins*`, `uninstall*`, `setup*`, `update*`, `install*`, `vc_redist*`, `vcredist*` ; un dossier présent deux fois (doublon) n'est jamais un conflit ; chemins `\\` jamais scannés ; gagnant = 1re entrée dans l'ordre Système puis Utilisateur ; 30 noms au plus par entrée. Commande séparée `analyze_conflicts` (scan disque) pour ne pas ralentir `get_registry_paths`.
- **Règle à appliquer :** un conflit signalé est un indice (le gagnant est celui que Windows lance, les masqués ne sont lancés que par chemin complet) ; élargir la liste de bruit plutôt que d'afficher des faux positifs.

### Script PowerShell généré (setup-env.ps1)
- **Problème / Erreur :** `[Environment]::SetEnvironmentVariable` écrit TOUJOURS du REG_SZ : un `%USERPROFILE%` laissé tel quel ne serait jamais développé par Windows ; PowerShell traite aussi ‘ ’ ‚ ‛ comme des apostrophes (une valeur pouvait fermer sa chaîne et injecter du code) ; le nom du profil pouvait fermer le commentaire `<# #>` ; un .ps1 sans BOM casse les accents en PS 5.1.
- **Solution retenue :** le script développe les `%VAR%` à l'exécution (`ExpandEnvironmentVariables`) ; `ps_quote` double chaque apostrophe, y compris typographiques ; `comment_safe` ne garde que lettres/chiffres/` -_.():` ; fichier écrit en UTF-8 AVEC BOM et CRLF (`ps_file_bytes`) ; `SupportsShouldProcess` (-WhatIf) ; `-OnlyExisting` ; sauvegarde JSON dans `%TEMP%` hors -WhatIf ; USER uniquement, jamais HKLM ; mêmes exclusions de secrets que l'export JSON. Validé : `Parser::ParseFile` 0 erreur et exécution `-WhatIf`.
- **Règle à appliquer :** toute donnée du Registre insérée dans un script généré passe par `ps_quote` ; valider un script généré avec le Parser PowerShell ET `-WhatIf` avant de le livrer ; l'application réelle du script (écriture) n'a été validée que par lecture du code : la tester sur une machine/VM jetable.

### Édition de fichiers Rust par shell : citations
- **Problème / Erreur :** `perl -0pi -e 's/.../.../s'` avec du code Rust contenant `/`, `"`, `'` dans le shell → « Unknown regexp modifier », test non ajouté.
- **Solution retenue :** écrire le bloc dans un fichier (Write) puis l'insérer avec `T=fichier perl -0pi -e 'BEGIN{open F,"<",$ENV{T}; local $/; $t=<F>} s/\n\}\s*\z/\n\n$t}\n/s'`.
- **Règle à appliquer :** jamais de code multi-lignes dans un `-e` ; préférer Edit (ancre unique) ou l'insertion depuis un fichier.

### Release : cohérence de version et fichiers de test
- **Problème / Erreur :** un tag `v0.2.0` sur des manifestes encore en `0.1.0` : l'exécutable aurait affiché `v0.1.0` (la version de l'UI vient de `tauri.conf.json` via `app.getVersion()`).
- **Solution retenue :** `npm version X.Y.Z --no-git-tag-version` (met à jour `package.json` et `package-lock.json`) + `sed '0,/^version = "A"/s//version = "X.Y.Z"/'` sur `Cargo.toml` + même remplacement dans `tauri.conf.json` ; `Cargo.lock` suit au prochain `cargo test` ; commit `chore(release)` AVANT le build, tag APRÈS le build.
- **Règle à appliquer :** le binaire doit être construit à partir du commit qui porte le tag ; nettoyer `%APPDATA%\com.txfai.devenvdoctor\` (sauvegardes et exports de test) avant de rendre la main, mais seulement après avoir comparé l'empreinte du Registre à l'état d'origine.

### Clippy 1.99 : chunks_exact(2)
- **Problème / Erreur :** `clippy::chunks_exact_to_as_chunks` sur `bytes.chunks_exact(2)`.
- **Solution retenue :** `bytes.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c))`.
- **Règle à appliquer :** lancer `cargo clippy --all-targets` avant le commit ; `sed` avec `\c` dans le remplacement efface le backslash : éditer `CLAUDE.md` avec Edit, pas avec sed.

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

### i18n : langue globale, tests et validation UI
- **Problème / Erreur :** la langue du backend est un `AtomicBool` global : un test EN lancé en parallèle casse les tests qui assertent du français. Et dans la fenêtre, `SendKeys {DOWN}` ne change pas le `<select>` (la liste s'ouvre sans valider ; `SendKeys` vise la fenêtre au premier plan).
- **Solution retenue :** `LANG_LOCK` (dans `i18n.rs`, `#[cfg(test)]`) pris par tout test qui lit un message, avec `set_english(false)` au début et au retour à FR après un test EN. Pour la fenêtre : UI Automation, `ExpandCollapsePattern.Expand()` sur le ComboBox puis `SelectionItemPattern.Select()` sur l'élément de liste (les onglets `role=tab` n'ont pas `InvokePattern` : utiliser `SelectionItemPattern`). Remettre FR avant de fermer (le localStorage du WebView est partagé avec l'app installée).
- **Règle à appliquer :** pas de Python sur cette machine : scripts de remplacement en Node (`String.raw` ne supporte pas les backticks du contenu ; passer en chaînes normales ou éditer à la main). Vérifier le registre (hash HKCU+HKLM avant/après) quand on pilote la vraie fenêtre.
