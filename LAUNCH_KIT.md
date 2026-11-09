# LAUNCH_KIT — DevEnv Doctor v0.2.0

Textes prêts à publier. Avant de poster : relire, adapter le ton, joindre **`assets/demo.gif`** (1,8 Mo, ~8 s : média principal de toutes les publications) ou, à défaut, une capture (`assets/ui-preview.png`, `assets/ui-diff.png`) et **respecter les règles d'auto-promotion de chaque communauté** (flair « Show & Tell » / « Project », pas de spam multi-posts le même jour, répondre aux commentaires).

- Repo : https://github.com/TxF-AI/devenv-doctor
- Release : https://github.com/TxF-AI/devenv-doctor/releases/tag/v0.2.0 (`devenv-doctor.exe`, 4,5 Mo)
- SHA-256 : `68E2E215D1A2B2621E404A4F0B3AF56F67F0DD4A86AB485D8070F1CD0086F986`

> Chiffres à ne pas gonfler : l'exécutable fait 4,5 Mo ; la mémoire utilisée n'est pas mesurée en release (l'interface passe par WebView2, pas par un Chromium embarqué). Ne pas annoncer de chiffre de RAM sans l'avoir mesuré.

---

## 1. Reddit — Show & Tell

**Subreddits :** `r/rust` (flair *project*), `r/windows`, `r/programming` (lien vers le repo, pas de texte promo), `r/CoolGithubProjects`. Un subreddit par jour, adapter l'intro.

**Média :** joindre `assets/demo.gif` (upload natif Reddit, ou lien image : https://github.com/TxF-AI/devenv-doctor/raw/main/assets/demo.gif).

**Titre (au choix)**
- `I was tired of a 6000-character PATH full of dead folders, so I wrote a small Rust/Tauri tool to clean it safely`
- `My Windows PATH was a mess after years of Python/Node/Java installs — so I built DevEnv Doctor (Rust + Tauri v2, 4.5 MB)`

**Corps**

Hi all,

After years of installing Python, Node, Java, Git, SDKs and uninstalling half of them, my Windows `PATH` was a graveyard: folders that no longer exist, duplicates, and several `python.exe` / `node.exe` hiding each other. Editing it in the "Environment Variables" dialog is painful and one wrong click can break your toolchain.

So I wrote **DevEnv Doctor**: it reads `HKCU` and `HKLM` directly from the registry, shows you what is wrong, and fixes it with a preview, an automatic backup and one-click restore.

**What it does**
- Detects **orphan entries** (folders that no longer exist) and **duplicates**, for User and System PATH. Entries it can't be sure about (unresolved variable, network path) are left alone.
- **Version conflicts**: finds binaries with the same name in several PATH folders and tells you which one Windows actually runs.
- **PATH length diagnostics** with warnings before the 1024 (`setx`), 2048 and 8192 limits, plus a **compressor** that swaps `C:\Users\<you>\...` for `%USERPROFILE%`, `%LOCALAPPDATA%`… only when it makes the entry shorter.
- ⬆/⬇ to change priority between entries.
- **Safety first**: Git-style diff modal before any write, JSON backup created automatically, one-click restore, rights checked before the first write (System PATH needs admin), `WM_SETTINGCHANGE` broadcast so new terminals see the change.
- **Dotfiles export**: JSON profiles (no secrets, no username) and a standalone `setup-env.ps1` (`-WhatIf` supported) to rebuild an environment on a fresh PC.

**Why Rust + Tauri v2**: I wanted a single portable ~4.5 MB `.exe`, direct registry access (`winreg`), and no bundled Chromium. 49 unit tests, `clippy` clean, MIT licensed, nothing is sent to the Internet.

**Download / verify** (the exe is not code-signed, so SmartScreen will warn; compare the hash):
https://github.com/TxF-AI/devenv-doctor/releases/tag/v0.2.0
`SHA-256: 68E2E215D1A2B2621E404A4F0B3AF56F67F0DD4A86AB485D8070F1CD0086F986`

It's v0.2.0 and I'd really like **feedback**: edge cases in your PATH that confuse it, features you'd want (per-project profiles? PATH history?), and any bug on your setup. Issues and PRs welcome.

---

## 2. Hacker News — Show HN

**Titre**
`Show HN: DevEnv Doctor – Safely clean your Windows PATH (Rust/Tauri)`

**URL :** https://github.com/TxF-AI/devenv-doctor

**Média :** HN n'héberge pas d'images : `assets/demo.gif` s'affiche en tête de la section « Aperçu » du README du dépôt. Ajouter dans le premier commentaire : `Demo (8 s): https://github.com/TxF-AI/devenv-doctor/raw/main/assets/demo.gif`

**Premier commentaire (à poster juste après la soumission)**

Hi HN, author here.

Windows PATH hygiene is a small but recurring pain: every SDK installer appends a folder, uninstallers leave dead entries, and the result can approach the 2048/8192 character limits while shadowing binaries (`python.exe`, `node.exe`) in ways that are hard to see in the built-in dialog.

DevEnv Doctor reads the PATH values straight from `HKCU\Environment` and `HKLM\...\Session Manager\Environment`, classifies each entry (valid / orphan / duplicate / version conflict) and applies fixes with a diff preview, an automatic JSON backup and one-click restore.

A few design choices that might interest you:
- **Types matter.** A PATH can be `REG_SZ` (no `%VAR%` expansion) or `REG_EXPAND_SZ`. I read the raw value and only expand when Windows would, and I write back the original type so `%VAR%` entries keep working.
- **Never make it worse.** The compressor only substitutes `%USERPROFILE%`/`%LOCALAPPDATA%`/`%APPDATA%`/`%windir%` when the result is strictly shorter (an early design could lengthen `%SystemRoot%`; I caught it during testing and added a "never lengthen" guard).
- **No partial writes.** Rights for every scope to modify are checked before the first write; System PATH changes need admin and fail early with a clear message.
- **Tested against a throwaway key**, never the real registry, in unit tests (49 of them).
- **Tauri v2, not Electron**: one ~4.5 MB portable exe using the system WebView2. I haven't benchmarked memory, so I won't quote a number.
- Everything is local; nothing is sent anywhere. MIT.

Limitations: Windows only, unsigned binary (SmartScreen warning; SHA-256 is in the release notes), and process-level environments already running keep their old PATH until restarted (new terminals get it via `WM_SETTINGCHANGE`).

I'd love feedback on edge cases and on which safety guarantees you'd want before trusting a tool with your PATH.

---

## 3. X (Twitter) & LinkedIn

**Média à joindre :** `assets/demo.gif` (X : GIF natif ; LinkedIn : ajouter comme image/GIF au post, 1er commentaire = lien du repo). En secours : `assets/ui-preview.png` + `assets/ui-diff.png`.

**X (≤ 280 caractères, version courte)**

```
My Windows PATH was 6000 chars of dead folders and shadowed binaries 😅

So I built DevEnv Doctor 🩺 (Rust + Tauri v2, 4.5 MB):
🧹 orphans & duplicates
⚠️ version conflicts
📏 PATH compressor
🛡️ diff preview + auto backup

MIT 👉 github.com/TxF-AI/devenv-doctor
#RustLang #Tauri #BuildInPublic #OpenSource
```

**X (thread, 2ᵉ tweet)**
```
Safety was the whole point: Git-style diff before any write, automatic JSON backup, one-click restore, rights checked before the first write, WM_SETTINGCHANGE broadcast so new terminals see it. 49 tests. Feedback welcome 🙏
```

**LinkedIn (FR)**

```
🩺 J'ai codé un outil pour réparer le PATH de Windows — et je le publie en open source.

Après des années d'installations (Python, Node, Java, Git…), mon PATH contenait des dossiers disparus, des doublons et des binaires qui se masquaient entre eux. L'éditer à la main, c'est risqué.

DevEnv Doctor (v0.2.0) :
🧹 détecte les entrées orphelines et les doublons (Utilisateur et Système)
⚠️ repère les conflits de version (quel python.exe Windows lance vraiment ?)
📏 mesure la longueur du PATH et le compresse avec %USERPROFILE%, %LOCALAPPDATA%…
🛡️ aperçu « diff », sauvegarde automatique, restauration en un clic
📜 export de profils (JSON) et d'un script PowerShell pour reconfigurer un PC vierge

Rust + Tauri v2 : un seul exécutable de 4,5 Mo, rien n'est envoyé sur Internet, licence MIT.

👉 https://github.com/TxF-AI/devenv-doctor

Je cherche des retours : quels cas limites de PATH rencontrez-vous ?

#RustLang #Tauri #BuildInPublic #OpenSource #Windows #DevTools
```

**LinkedIn (EN, variante courte)**
```
Built a small Rust/Tauri v2 tool to safely clean the Windows PATH: orphan & duplicate detection, version-conflict finder, PATH compressor, diff preview, auto backup + one-click restore. 4.5 MB, MIT, local only.
github.com/TxF-AI/devenv-doctor — feedback very welcome!
#RustLang #Tauri #BuildInPublic #OpenSource
```

---

## 4. Discord — #showcase

**Salons :** Discord officiel Tauri (`#showcase`), DevFR (salon projets), serveurs Rust FR/EN. Lire les règles du salon avant de poster.

**Média :** joindre `assets/demo.gif` directement dans le message (limite Discord largement respectée).

**Message (EN)**
```
🩺 **DevEnv Doctor** — a Tauri v2 app to safely clean your Windows PATH
Reads HKCU/HKLM from the registry, flags orphans, duplicates and version conflicts, compresses the PATH (%USERPROFILE%…), and fixes it with a diff preview + auto backup + one-click restore. ~4.5 MB portable exe, MIT, 100% local.
🔗 https://github.com/TxF-AI/devenv-doctor
Would love feedback on the UI and on PATH edge cases! 🙏
```

**Message (FR, DevFR)**
```
🩺 **DevEnv Doctor** — nettoyeur de PATH Windows (Rust + Tauri v2)
Lit le Registre (HKCU/HKLM), repère orphelins, doublons et conflits de version, compresse le PATH et corrige avec aperçu « diff », sauvegarde auto et restauration en 1 clic. Exe portable ~4,5 Mo, MIT, 100 % local.
🔗 https://github.com/TxF-AI/devenv-doctor
Vos retours sont les bienvenus (UI, cas limites de PATH) !
```
