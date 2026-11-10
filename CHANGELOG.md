# Changelog

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
