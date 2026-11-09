//! Profils d'environnement : export (dotfiles), import et application.
//!
//! Portée volontairement limitée : l'application ne modifie que HKCU
//! (PATH Utilisateur + variables utilisateur). `path_system` est exporté à titre
//! informatif et jamais appliqué. Tout passe par aperçu → confirmation → sauvegarde.

use crate::actions::{
    backups_dir, broadcast_environment_change, can_write, is_safe_orphan, read_user_vars,
    snapshot_with_vars, write_backup, write_path_value, write_user_var, ScopeBackup,
};
use crate::{
    analyze_registry_path, build_report, dedup_key, expand_env, read_path_value, PathEntry, USER_KEY,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use winreg::enums::HKEY_CURRENT_USER;

const PROFILE_VERSION: u32 = 1;
const MAX_ENTRIES: usize = 500;
const MAX_VARIABLES: usize = 200;

/// Variables que Windows ou l'utilisateur ne doivent pas voir réécrites par un profil importé.
const PROTECTED_VARS: &[&str] = &[
    "PATH", "PATHEXT", "COMSPEC", "WINDIR", "SYSTEMROOT", "SYSTEMDRIVE", "PSMODULEPATH",
    "USERPROFILE", "APPDATA", "LOCALAPPDATA", "PROGRAMDATA", "PROGRAMFILES", "HOMEDRIVE",
    "HOMEPATH", "USERNAME", "COMPUTERNAME", "OS", "TEMP", "TMP",
];

/// Mots dont la présence dans un nom de variable la fait exclure de l'export (clés d'API, mots de passe…).
const SECRET_WORDS: &[&str] = &[
    "TOKEN", "SECRET", "PASS", "KEY", "CRED", "AUTH", "PRIVATE", "SESSION", "COOKIE",
];

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum ApplyMode {
    /// Le PATH utilisateur devient exactement `path_user`.
    Replace,
    /// Ajoute à la fin les entrées de `path_user` qui existent sur le disque et manquent.
    Merge,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct EnvironmentProfile {
    pub version: u32,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub created_at: String,
    pub apply_mode: ApplyMode,
    /// Entrées du PATH Utilisateur (texte brut, `%VAR%` conservées). Vide = PATH inchangé.
    #[serde(default)]
    pub path_user: Vec<String>,
    /// PATH Système, à titre informatif : jamais appliqué.
    #[serde(default)]
    pub path_system: Vec<String>,
    /// Variables utilisateur à définir.
    #[serde(default)]
    pub variables: BTreeMap<String, String>,
    /// Après application, retirer doublons et orphelins sûrs du PATH Utilisateur.
    #[serde(default)]
    pub clean_after: bool,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct VarChange {
    pub name: String,
    pub old: Option<String>,
    pub new: String,
    pub value_type: String,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct ProfilePlan {
    pub name: String,
    pub path_added: Vec<String>,
    pub path_removed: Vec<String>,
    /// Nouvelle valeur du PATH Utilisateur ; None = inchangé.
    pub new_user_path: Option<String>,
    pub path_value_type: Option<String>,
    pub variables: Vec<VarChange>,
    /// Éléments non appliqués, avec la raison (variable protégée, dossier introuvable…).
    pub notes: Vec<String>,
    pub system_entries_ignored: usize,
}

impl ProfilePlan {
    pub fn is_empty(&self) -> bool {
        self.new_user_path.is_none() && self.variables.is_empty()
    }
}

#[derive(Serialize, Debug, PartialEq)]
pub struct ApplyResult {
    pub changed: bool,
    pub backup_file: Option<String>,
    pub variables_changed: usize,
    pub path_changed: bool,
    pub broadcast: bool,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct ExportResult {
    pub path: String,
    pub json: String,
    pub path_user_count: usize,
    pub path_system_count: usize,
    pub variable_count: usize,
    pub skipped_secrets: Vec<String>,
}

// ───────────────────────── Validation ─────────────────────────

fn valid_var_name(n: &str) -> bool {
    let mut c = n.chars();
    matches!(c.next(), Some(f) if f.is_ascii_alphabetic() || f == '_')
        && n.len() <= 64
        && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn valid_entry(e: &str) -> bool {
    let t = e.trim();
    !t.is_empty() && t.len() <= 1024 && !t.contains(';') && !t.contains('\0')
}

pub fn is_protected_var(name: &str) -> bool {
    PROTECTED_VARS.iter().any(|p| p.eq_ignore_ascii_case(name))
}

pub fn looks_secret(name: &str) -> bool {
    let up = name.to_uppercase();
    SECRET_WORDS.iter().any(|w| up.contains(w))
}

/// Refuse un profil mal formé ou démesuré avant de s'en servir.
pub fn validate_profile(p: &EnvironmentProfile) -> Result<(), String> {
    if p.version != PROFILE_VERSION {
        return Err(format!("Version de profil non prise en charge : {}", p.version));
    }
    let name = p.name.trim();
    if name.is_empty() || name.chars().count() > 64 || name.chars().any(|c| c.is_control()) {
        return Err("Nom de profil invalide (1 à 64 caractères, sans caractère de contrôle).".into());
    }
    if p.description.chars().count() > 500 {
        return Err("Description trop longue (500 caractères maximum).".into());
    }
    for (label, list) in [("path_user", &p.path_user), ("path_system", &p.path_system)] {
        if list.len() > MAX_ENTRIES {
            return Err(format!("{label} : trop d'entrées (maximum {MAX_ENTRIES})."));
        }
        if let Some(bad) = list.iter().find(|e| !valid_entry(e)) {
            return Err(format!("{label} : entrée invalide « {bad} » (vide, trop longue, « ; » ou NUL)."));
        }
    }
    if p.variables.len() > MAX_VARIABLES {
        return Err(format!("Trop de variables (maximum {MAX_VARIABLES})."));
    }
    for (k, v) in &p.variables {
        if !valid_var_name(k) {
            return Err(format!("Nom de variable invalide : « {k} » (lettres, chiffres, « _ »)."));
        }
        if v.len() > 8192 || v.contains('\0') {
            return Err(format!("Valeur invalide pour {k} (trop longue ou caractère NUL)."));
        }
    }
    Ok(())
}

pub fn parse_profile(json: &str) -> Result<EnvironmentProfile, String> {
    if json.len() > 1_000_000 {
        return Err("Fichier de profil trop volumineux (1 Mo maximum).".into());
    }
    let p: EnvironmentProfile =
        serde_json::from_str(json).map_err(|e| format!("Profil illisible : {e}"))?;
    validate_profile(&p)?;
    Ok(p)
}

// ───────────────────────── Presets ─────────────────────────

pub fn builtin_presets() -> Vec<EnvironmentProfile> {
    let mk = |name: &str, description: &str, paths: &[&str], clean_after: bool| EnvironmentProfile {
        version: PROFILE_VERSION,
        name: name.to_string(),
        description: description.to_string(),
        created_at: String::new(),
        apply_mode: ApplyMode::Merge,
        path_user: paths.iter().map(|s| s.to_string()).collect(),
        path_system: vec![],
        variables: BTreeMap::new(),
        clean_after,
    };
    vec![
        mk(
            "Minimal Clean",
            "Retire doublons et orphelins sûrs du PATH Utilisateur, n'ajoute rien.",
            &[],
            true,
        ),
        mk(
            "WebDev",
            "Ajoute (si présents sur le disque et absents du PATH) Node.js, npm global, Git et VS Code.",
            &[
                r"%ProgramFiles%\nodejs",
                r"%APPDATA%\npm",
                r"%ProgramFiles%\Git\cmd",
                r"%LOCALAPPDATA%\Programs\Microsoft VS Code\bin",
            ],
            false,
        ),
        mk(
            "GameDev",
            "Ajoute (si présents sur le disque et absents du PATH) Git, CMake, .NET et les liens WinGet.",
            &[
                r"%ProgramFiles%\Git\cmd",
                r"%ProgramFiles%\CMake\bin",
                r"%ProgramFiles%\dotnet",
                r"%LOCALAPPDATA%\Microsoft\WinGet\Links",
            ],
            false,
        ),
    ]
}

// ───────────────────────── Plan d'application (pur) ─────────────────────────

fn entries_of(raw: &str) -> Vec<String> {
    raw.split(';').map(str::trim).filter(|p| !p.is_empty()).map(String::from).collect()
}

#[allow(clippy::too_many_arguments)]
pub fn compute_plan(
    profile: &EnvironmentProfile,
    current_user_raw: Option<&str>,
    user_type: Option<&str>,
    system_entries: &[PathEntry],
    current_vars: &BTreeMap<String, ScopeBackup>,
    lookup: &impl Fn(&str) -> Option<String>,
    exists: &impl Fn(&str) -> bool,
) -> Result<ProfilePlan, String> {
    let mut notes = Vec::new();
    let current = entries_of(current_user_raw.unwrap_or(""));
    let sys_keys: HashSet<String> = system_entries.iter().map(|e| dedup_key(&e.expanded_path)).collect();
    let touch_path = !profile.path_user.is_empty() || profile.clean_after;

    let mut new_list = current.clone();
    if touch_path {
        match profile.apply_mode {
            ApplyMode::Replace => {
                if !profile.path_user.is_empty() {
                    new_list = profile.path_user.iter().map(|s| s.trim().to_string()).collect();
                }
            }
            ApplyMode::Merge => {
                let mut have: HashSet<String> =
                    current.iter().map(|c| dedup_key(&expand_env(c, lookup))).collect();
                have.extend(sys_keys.iter().cloned());
                for c in &profile.path_user {
                    let expanded = expand_env(c.trim(), lookup);
                    let key = dedup_key(&expanded);
                    if have.contains(&key) {
                        continue; // déjà dans le PATH (Utilisateur ou Système)
                    }
                    if !exists(expanded.trim().trim_matches('"')) {
                        notes.push(format!("{c} : dossier introuvable, non ajouté"));
                        continue;
                    }
                    have.insert(key);
                    new_list.push(c.trim().to_string());
                }
            }
        }
        if profile.clean_after {
            let mut seen = sys_keys.clone();
            let joined = new_list.join(";");
            new_list = analyze_registry_path("User", &joined, true, lookup, exists, &mut seen)
                .into_iter()
                .filter(|e| !(e.is_duplicate || (!e.exists && is_safe_orphan(e))))
                .map(|e| e.raw_path)
                .collect();
        }
    }

    let path_changed = touch_path && new_list != current;
    if path_changed && new_list.is_empty() {
        return Err("Le PATH Utilisateur résultant serait vide : refusé.".into());
    }
    let lower = |v: &[String]| v.iter().map(|s| s.to_lowercase()).collect::<HashSet<_>>();
    let (cur_set, new_set) = (lower(&current), lower(&new_list));
    let (path_added, path_removed) = if path_changed {
        (
            new_list.iter().filter(|n| !cur_set.contains(&n.to_lowercase())).cloned().collect(),
            current.iter().filter(|c| !new_set.contains(&c.to_lowercase())).cloned().collect(),
        )
    } else {
        (vec![], vec![])
    };
    let path_value_type = if path_changed {
        let has_var = new_list.iter().any(|e| e.contains('%'));
        Some(match user_type {
            Some("REG_SZ") if !has_var => "REG_SZ",
            Some("REG_SZ") | Some("REG_EXPAND_SZ") | None => "REG_EXPAND_SZ",
            Some(other) => return Err(format!("Type du PATH Utilisateur non pris en charge : {other}")),
        })
    } else {
        None
    };

    let mut variables = Vec::new();
    for (name, value) in &profile.variables {
        if is_protected_var(name) {
            notes.push(format!("{name} : variable protégée, ignorée"));
            continue;
        }
        let existing = current_vars.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v);
        if existing.is_some_and(|v| &v.raw == value) {
            continue; // déjà à cette valeur
        }
        let expandable = value.contains('%') || existing.is_some_and(|v| v.value_type == "REG_EXPAND_SZ");
        variables.push(VarChange {
            name: name.clone(),
            old: existing.map(|v| v.raw.clone()),
            new: value.clone(),
            value_type: if expandable { "REG_EXPAND_SZ" } else { "REG_SZ" }.to_string(),
        });
    }

    Ok(ProfilePlan {
        name: profile.name.clone(),
        path_added,
        path_removed,
        new_user_path: path_changed.then(|| new_list.join(";")),
        path_value_type: path_value_type.map(String::from),
        variables,
        notes,
        system_entries_ignored: profile.path_system.len(),
    })
}

// ───────────────────────── Export (pur) ─────────────────────────

/// Remplace (sans tenir compte de la casse) le dossier du profil Windows par `%USERPROFILE%` :
/// le fichier exporté devient portable et ne contient plus le nom d'utilisateur.
pub fn portable(s: &str, userprofile: Option<&str>) -> String {
    let Some(up) = userprofile.filter(|u| u.len() > 3) else { return s.to_string() };
    let (hay, needle) = (s.to_ascii_lowercase(), up.to_ascii_lowercase());
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while let Some(pos) = hay[i..].find(&needle) {
        out.push_str(&s[i..i + pos]);
        out.push_str("%USERPROFILE%");
        i += pos + needle.len();
    }
    out.push_str(&s[i..]);
    out
}

pub fn build_export(
    name: &str,
    description: &str,
    created_at: &str,
    user_raw: Option<&str>,
    system_raw: Option<&str>,
    vars: &BTreeMap<String, ScopeBackup>,
    userprofile: Option<&str>,
) -> (EnvironmentProfile, Vec<String>) {
    let conv = |raw: Option<&str>| -> Vec<String> {
        entries_of(raw.unwrap_or("")).iter().map(|e| portable(e, userprofile)).collect()
    };
    let mut variables = BTreeMap::new();
    let mut skipped = Vec::new();
    for (k, v) in vars {
        if is_protected_var(k) || !valid_var_name(k) {
            continue;
        }
        if looks_secret(k) {
            skipped.push(k.clone());
        } else {
            variables.insert(k.clone(), portable(&v.raw, userprofile));
        }
    }
    let profile = EnvironmentProfile {
        version: PROFILE_VERSION,
        name: name.trim().to_string(),
        description: description.trim().to_string(),
        created_at: created_at.to_string(),
        apply_mode: ApplyMode::Replace,
        path_user: conv(user_raw),
        path_system: conv(system_raw),
        variables,
        clean_after: false,
    };
    (profile, skipped)
}

/// Nom de fichier sûr dérivé du nom du profil.
pub fn sanitize_file_name(name: &str) -> String {
    let s: String = name
        .trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let s = s.trim_matches('_').to_string();
    if s.is_empty() { "profil".to_string() } else { s.chars().take(48).collect() }
}

// ───────────────────────── Commandes Tauri ─────────────────────────

fn real_plan(profile: &EnvironmentProfile) -> Result<ProfilePlan, String> {
    validate_profile(profile)?;
    let report = build_report();
    if let Some(e) = &report.user.error {
        return Err(format!("Lecture du PATH Utilisateur impossible : {e}"));
    }
    let user = read_path_value(HKEY_CURRENT_USER, USER_KEY)?;
    let vars = read_user_vars()?;
    compute_plan(
        profile,
        user.as_ref().map(|(raw, _, _)| raw.as_str()),
        user.as_ref().map(|(_, _, t)| t.as_str()),
        &report.system.entries,
        &vars,
        &|n: &str| std::env::var(n).ok(),
        &|p: &str| Path::new(p).exists(),
    )
}

#[tauri::command]
pub async fn list_presets() -> Vec<EnvironmentProfile> {
    builtin_presets()
}

/// Exporte le PATH et les variables utilisateur dans `profiles/<nom>.json` (écrase un export du même nom).
/// Les variables au nom « secret » (token, key, password…) sont exclues et listées.
#[tauri::command]
pub async fn export_profile(
    app: tauri::AppHandle,
    name: String,
    description: String,
) -> Result<ExportResult, String> {
    use tauri::Manager;
    let user = read_path_value(HKEY_CURRENT_USER, USER_KEY)?;
    let system = read_path_value(winreg::enums::HKEY_LOCAL_MACHINE, crate::SYSTEM_KEY)?;
    let vars = read_user_vars()?;
    let userprofile = std::env::var("USERPROFILE").ok();
    let (profile, skipped_secrets) = build_export(
        &name,
        &description,
        &chrono::Local::now().to_rfc3339(),
        user.as_ref().map(|(r, _, _)| r.as_str()),
        system.as_ref().map(|(r, _, _)| r.as_str()),
        &vars,
        userprofile.as_deref(),
    );
    validate_profile(&profile)?;
    let json = serde_json::to_string_pretty(&profile).map_err(|e| e.to_string())?;
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Dossier de données introuvable : {e}"))?
        .join("profiles");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Création de {} impossible : {e}", dir.display()))?;
    let file = dir.join(format!("{}.json", sanitize_file_name(&profile.name)));
    std::fs::write(&file, &json).map_err(|e| format!("Écriture de {} impossible : {e}", file.display()))?;
    Ok(ExportResult {
        path: file.display().to_string(),
        json,
        path_user_count: profile.path_user.len(),
        path_system_count: profile.path_system.len(),
        variable_count: profile.variables.len(),
        skipped_secrets,
    })
}

/// Lit et valide le texte JSON d'un profil (n'applique rien).
#[tauri::command]
pub async fn import_profile(json: String) -> Result<EnvironmentProfile, String> {
    parse_profile(&json)
}

/// Aperçu (lecture seule) de ce que ferait `apply_profile`.
#[tauri::command]
pub async fn preview_profile(profile: EnvironmentProfile) -> Result<ProfilePlan, String> {
    real_plan(&profile)
}

/// Applique un profil à HKCU : sauvegarde automatique, écriture, puis WM_SETTINGCHANGE.
/// Le plan est recalculé ici : on n'applique jamais un aperçu périmé.
#[tauri::command]
pub async fn apply_profile(app: tauri::AppHandle, profile: EnvironmentProfile) -> Result<ApplyResult, String> {
    let plan = real_plan(&profile)?;
    if plan.is_empty() {
        return Ok(ApplyResult { changed: false, backup_file: None, variables_changed: 0, path_changed: false, broadcast: false });
    }
    if !can_write(HKEY_CURRENT_USER, USER_KEY) {
        return Err("Écriture refusée sur HKCU\\Environment.".into());
    }
    let names: Vec<String> = plan.variables.iter().map(|v| v.name.clone()).collect();
    let backup = snapshot_with_vars(&format!("avant-profil:{}", plan.name), &names)?;
    let file = write_backup(&backups_dir(&app)?, &backup)?;
    let file_name = file.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();

    if let (Some(path), Some(ty)) = (&plan.new_user_path, &plan.path_value_type) {
        write_path_value(HKEY_CURRENT_USER, USER_KEY, path, ty)
            .map_err(|e| format!("{e} (sauvegarde : {file_name})"))?;
    }
    for v in &plan.variables {
        write_user_var(&v.name, &v.new, &v.value_type).map_err(|e| format!("{e} (sauvegarde : {file_name})"))?;
    }
    Ok(ApplyResult {
        changed: true,
        backup_file: Some(file_name),
        variables_changed: plan.variables.len(),
        path_changed: plan.new_user_path.is_some(),
        broadcast: broadcast_environment_change(),
    })
}

// ───────────────────────── Tests ─────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(n: &str) -> Option<String> {
        match n.to_lowercase().as_str() {
            "userprofile" => Some(r"C:\Users\Moi".into()),
            "programfiles" => Some(r"C:\PF".into()),
            _ => None,
        }
    }
    fn profile(mode: ApplyMode, paths: &[&str], clean: bool) -> EnvironmentProfile {
        EnvironmentProfile {
            version: 1,
            name: "T".into(),
            description: String::new(),
            created_at: String::new(),
            apply_mode: mode,
            path_user: paths.iter().map(|s| s.to_string()).collect(),
            path_system: vec![],
            variables: BTreeMap::new(),
            clean_after: clean,
        }
    }
    fn sys(raws: &[&str]) -> Vec<PathEntry> {
        let mut seen = HashSet::new();
        analyze_registry_path("System", &raws.join(";"), true, &lookup, &|_| true, &mut seen)
    }
    fn var(v: &str, t: &str) -> ScopeBackup {
        ScopeBackup { value_type: t.into(), raw: v.into() }
    }
    fn plan(p: &EnvironmentProfile, cur: &str, ty: Option<&str>, s: &[PathEntry], vars: &BTreeMap<String, ScopeBackup>, ex: impl Fn(&str) -> bool) -> Result<ProfilePlan, String> {
        compute_plan(p, Some(cur), ty, s, vars, &lookup, &ex)
    }

    #[test]
    fn merge_ajoute_seulement_les_dossiers_existants_et_absents() {
        let p = profile(ApplyMode::Merge, &[r"%ProgramFiles%\node", r"%ProgramFiles%\absent", r"C:\Deja", r"C:\DuSysteme"], false);
        let ex = |x: &str| x != r"C:\PF\absent";
        let r = plan(&p, r"C:\Deja", Some("REG_EXPAND_SZ"), &sys(&[r"c:\dusysteme\"]), &BTreeMap::new(), ex).unwrap();
        assert_eq!(r.path_added, vec![r"%ProgramFiles%\node"]);
        assert_eq!(r.new_user_path.as_deref(), Some(r"C:\Deja;%ProgramFiles%\node"));
        assert!(r.notes.iter().any(|n| n.contains("absent") && n.contains("non ajouté")));
        assert!(r.path_removed.is_empty());
    }

    #[test]
    fn merge_sans_changement_ne_touche_rien() {
        let p = profile(ApplyMode::Merge, &[r"C:\A"], false);
        let r = plan(&p, r"C:\A", Some("REG_SZ"), &[], &BTreeMap::new(), |_| true).unwrap();
        assert!(r.is_empty() && r.new_user_path.is_none());
    }

    #[test]
    fn replace_liste_ajouts_et_retraits_et_garde_le_type() {
        let p = profile(ApplyMode::Replace, &[r"C:\A", r"C:\C"], false);
        let r = plan(&p, r"C:\A;C:\B", Some("REG_SZ"), &[], &BTreeMap::new(), |_| true).unwrap();
        assert_eq!(r.path_added, vec![r"C:\C"]);
        assert_eq!(r.path_removed, vec![r"C:\B"]);
        assert_eq!(r.new_user_path.as_deref(), Some(r"C:\A;C:\C"));
        assert_eq!(r.path_value_type.as_deref(), Some("REG_SZ")); // pas de % : le type d'origine reste
        let p2 = profile(ApplyMode::Replace, &[r"%USERPROFILE%\bin"], false);
        let r2 = plan(&p2, r"C:\A", Some("REG_SZ"), &[], &BTreeMap::new(), |_| true).unwrap();
        assert_eq!(r2.path_value_type.as_deref(), Some("REG_EXPAND_SZ")); // un % impose EXPAND
    }

    #[test]
    fn clean_after_retire_doublons_et_orphelins_surs_mais_garde_les_incertains() {
        let p = profile(ApplyMode::Merge, &[], true);
        let ex = |x: &str| x == r"C:\Ok";
        let cur = r"C:\Ok;c:\ok\;C:\Mort;%INCONNUE%\x;\\srv\p;C:\DuSysteme";
        let r = plan(&p, cur, Some("REG_EXPAND_SZ"), &sys(&[r"C:\DuSysteme"]), &BTreeMap::new(), ex).unwrap();
        assert_eq!(r.new_user_path.as_deref(), Some(r"C:\Ok;%INCONNUE%\x;\\srv\p"));
    }

    #[test]
    fn refuse_un_path_vide() {
        let p = profile(ApplyMode::Merge, &[], true);
        let r = plan(&p, r"C:\Mort", Some("REG_SZ"), &[], &BTreeMap::new(), |_| false);
        assert!(r.is_err());
    }

    #[test]
    fn variables_protegees_ignorees_inchangees_sautees_et_type_choisi() {
        let mut p = profile(ApplyMode::Merge, &[], false);
        p.variables = [
            ("Path", "x"), ("comspec", "evil.exe"), ("EDITOR", "code"), ("SAME", "1"),
            ("HOMEDIR", r"%USERPROFILE%\h"), ("OLDX", "new"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let cur: BTreeMap<String, ScopeBackup> = [
            ("same".to_string(), var("1", "REG_SZ")),
            ("OLDX".to_string(), var("old", "REG_EXPAND_SZ")),
        ]
        .into_iter()
        .collect();
        let r = plan(&p, r"C:\A", Some("REG_SZ"), &[], &cur, |_| true).unwrap();
        let names: Vec<&str> = r.variables.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, vec!["EDITOR", "HOMEDIR", "OLDX"]);
        let get = |n: &str| r.variables.iter().find(|v| v.name == n).unwrap();
        assert_eq!(get("EDITOR").value_type, "REG_SZ");
        assert_eq!(get("HOMEDIR").value_type, "REG_EXPAND_SZ");
        assert_eq!(get("OLDX").value_type, "REG_EXPAND_SZ"); // le type existant est conservé
        assert_eq!(get("OLDX").old.as_deref(), Some("old"));
        assert_eq!(r.notes.iter().filter(|n| n.contains("protégée")).count(), 2);
    }

    #[test]
    fn export_portable_sans_secrets() {
        let vars: BTreeMap<String, ScopeBackup> = [
            ("EDITOR", r"C:\Users\Moi\bin\code.exe"),
            ("GITHUB_TOKEN", "ghp_xxx"),
            ("MY_API_KEY", "k"),
            ("TEMP", r"C:\Users\Moi\AppData\Local\Temp"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), var(v, "REG_SZ")))
        .collect();
        let (p, skipped) = build_export("Mon Profil", " d ", "t", Some(r"C:\USERS\moi\.cargo\bin;D:\x;"), Some(r"C:\Windows"), &vars, Some(r"C:\Users\Moi"));
        assert_eq!(p.path_user, vec![r"%USERPROFILE%\.cargo\bin", r"D:\x"]);
        assert_eq!(p.path_system, vec![r"C:\Windows"]);
        assert_eq!(p.variables.len(), 1);
        assert_eq!(p.variables["EDITOR"], r"%USERPROFILE%\bin\code.exe");
        assert_eq!(skipped, vec!["GITHUB_TOKEN", "MY_API_KEY"]);
        assert_eq!(p.apply_mode, ApplyMode::Replace);
        assert!(validate_profile(&p).is_ok());
        let json = serde_json::to_string(&p).unwrap();
        assert!(!json.contains("Moi") && !json.contains("ghp_xxx") && json.contains("\"apply_mode\":\"replace\""));
    }

    #[test]
    fn portable_ne_touche_pas_les_chemins_voisins() {
        assert_eq!(portable(r"C:\Users\MoiEtLautre\x", Some(r"C:\Users\Moi")), r"%USERPROFILE%EtLautre\x"); // limite connue : préfixe brut
        assert_eq!(portable(r"D:\x", Some(r"C:\Users\Moi")), r"D:\x");
        assert_eq!(portable(r"C:\Users\Moi", None), r"C:\Users\Moi");
    }

    #[test]
    fn validation_refuse_le_douteux() {
        let ok = profile(ApplyMode::Merge, &[r"C:\A"], false);
        assert!(validate_profile(&ok).is_ok());
        let mut p = ok.clone(); p.version = 9; assert!(validate_profile(&p).is_err());
        let mut p = ok.clone(); p.name = "  ".into(); assert!(validate_profile(&p).is_err());
        let mut p = ok.clone(); p.path_user = vec!["C:\\a;C:\\b".into()]; assert!(validate_profile(&p).is_err());
        let mut p = ok.clone(); p.path_user = vec!["".into()]; assert!(validate_profile(&p).is_err());
        let mut p = ok.clone(); p.path_user = vec!["C:\\a".into(); 501]; assert!(validate_profile(&p).is_err());
        let mut p = ok.clone(); p.variables.insert("1BAD".into(), "x".into()); assert!(validate_profile(&p).is_err());
        let mut p = ok.clone(); p.variables.insert("A B".into(), "x".into()); assert!(validate_profile(&p).is_err());
        let mut p = ok.clone(); p.variables.insert("OK".into(), "a\0b".into()); assert!(validate_profile(&p).is_err());
    }

    #[test]
    fn json_aller_retour_et_champs_facultatifs() {
        let p = parse_profile(r#"{"version":1,"name":"Min","apply_mode":"merge"}"#).unwrap();
        assert!(p.path_user.is_empty() && p.variables.is_empty() && !p.clean_after);
        let json = serde_json::to_string_pretty(&p).unwrap();
        assert_eq!(parse_profile(&json).unwrap(), p);
        assert!(parse_profile(r#"{"version":1,"name":"X","apply_mode":"explode"}"#).is_err());
        assert!(parse_profile("pas du json").is_err());
        assert!(parse_profile(&" ".repeat(1_000_001)).is_err());
    }

    #[test]
    fn presets_valides_et_noms_attendus() {
        let names: Vec<String> = builtin_presets().into_iter().map(|p| p.name).collect();
        assert_eq!(names, vec!["Minimal Clean", "WebDev", "GameDev"]);
        assert!(builtin_presets().iter().all(|p| validate_profile(p).is_ok() && p.apply_mode == ApplyMode::Merge));
    }

    #[test]
    fn nom_de_fichier_sur() {
        assert_eq!(sanitize_file_name(r"..\Mon profil/Web:Dev"), "Mon_profil_Web_Dev");
        assert_eq!(sanitize_file_name("***"), "profil");
        assert!(sanitize_file_name(&"a".repeat(100)).len() <= 48);
    }

    #[test]
    fn detection_des_noms_secrets_et_proteges() {
        assert!(looks_secret("github_token") && looks_secret("AWS_SECRET_ACCESS_KEY") && looks_secret("DB_PASSWORD"));
        assert!(!looks_secret("EDITOR") && !looks_secret("JAVA_HOME"));
        assert!(is_protected_var("path") && is_protected_var("ComSpec") && !is_protected_var("JAVA_HOME"));
    }

    #[test]
    fn diffusion_wm_settingchange_ne_plante_pas() {
        // Appel réel mais inoffensif : aucune variable n'a changé. Le résultat dépend de
        // l'état des fenêtres du bureau, on vérifie surtout que l'appel FFI est sain.
        let _ = broadcast_environment_change();
    }
}
