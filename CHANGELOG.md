# Changelog

## 0.4.0 (2026-11-10)

- **Copy AI diagnostic prompt**: an **AI diagnostic** button in the toolbar copies a Markdown summary of the PATH to the clipboard: HKCU / HKLM / effective length with risk level, number of entries, broken entries, duplicates and shadowed executables (with the winning and the shadowed path), admin status, followed by a ready-made task for an AI assistant (Claude Code, Cursor…). The prompt is always in English; the Windows user name is replaced by `<user>`. Nothing leaves the machine.
- New `diag.js` module (pure `buildPrompt` function).

## 0.3.1 (2026-11-10)

- **Text polish**: proper plurals for the character count (`character` / `characters`, `caractère` / `caractères`) in the PATH compression messages.
- **PATH compression note**: clearer wording explaining that Windows resolves the variables at launch, so the paths stay equivalent.
- **Documentation**: bilingual "Who is it for?" section in both READMEs.

## 0.3.0 (2026-11-10)

- **FR / EN language selector** (2026-11-09): the interface, the backend messages and the exported PowerShell script follow the chosen language. The choice is remembered; it defaults to the system language.
- **Documentation**: English README by default (French in `README.fr.md`), English demo GIF and screenshots, English issue templates.

## 0.2.0 (2026-11-09)

- **PATH compressor**: replaces `C:\Users\<name>`, `AppData\Local`, `AppData\Roaming` and the Windows folder with `%USERPROFILE%`, `%LOCALAPPDATA%`, `%APPDATA%` and `%windir%`, only when the entry gets shorter; user-specific variables are never written to the System PATH.
- **PATH length**: character count of HKCU, HKLM and the effective PATH, with risk indicators at the 1024 (`setx`), 2048 and 8192 thresholds.
- **Binary conflict detection**: "Version conflict" badge when several folders contain an `.exe`, `.cmd`, `.bat` or `.com` with the same name (for example several `python.exe`), showing the winning entry and the shadowed ones.
- **Reordering**: up / down buttons to change an entry's priority within its scope.
- **PowerShell export**: generates `setup-env.ps1` to set up a blank PC (user PATH and variables, `-WhatIf`, no administrator rights, no app installation).
- **`WM_SETTINGCHANGE` broadcast** after every write: new terminals see the change without restarting the session.
- Cyberpunk HUD interface redesign, MIT license.

## 0.1.0 (2026-11-09)

- First release: direct HKCU / HKLM PATH reading from the Registry, orphan and duplicate cleanup, restorable JSON backups, profiles and presets (`Minimal Clean`, `WebDev`, `GameDev`), dotfiles export without secrets or user name.
