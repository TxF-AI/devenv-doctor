# CLAUDE.md - Mémoire & Directives du Projet DevEnv Doctor

## Vue d'ensemble & Architecture
- **Projet :** DevEnv Doctor (Nettoyeur de %PATH% Windows, Profils Dev & Backup GitHub).
- **Stack cible :** Rust (Backend) + Tauri v2 + Web Dark Mode / Cyberpunk (Frontend).
- **Périmètre :** Manipulation sécurisée du Registre Windows (`HKCU\Environment` et `HKLM\SYSTEM\...`).

## Commandes clés (À compléter au fil de l'initialisation)
- Build / Dev / Test / Lint : à renseigner une fois le projet Tauri initialisé (rien n'est encore vérifié).

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

(Aucune entrée pour l'instant.)
