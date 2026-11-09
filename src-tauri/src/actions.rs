//! Actions qui MODIFIENT le Registre : sauvegarde, restauration, nettoyage.
//! Règles : sauvegarde automatique avant toute écriture, aperçu séparé de
//! l'exécution, on ne touche qu'à la valeur `Path` en conservant son type.

use crate::{
    build_report, read_path_value, PathEntry, RegistryPathReport, SYSTEM_KEY, USER_KEY,
};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tauri::Manager;
use winreg::enums::{
    HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_SET_VALUE, REG_EXPAND_SZ, REG_SZ,
};
use winreg::{RegKey, RegValue, HKEY};

/// Préfixe stable des erreurs de droits ; l'interface le détecte.
pub const ADMIN_REQUIRED: &str = "AdminRequired";
const BACKUP_VERSION: u32 = 1;

// ───────────────────────── Sauvegardes ─────────────────────────

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ScopeBackup {
    /// "REG_SZ" ou "REG_EXPAND_SZ".
    pub value_type: String,
    /// Texte exact de la valeur `Path`.
    pub raw: String,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Backup {
    pub version: u32,
    pub created_at: String,
    pub reason: String,
    /// `None` : la valeur n'existait pas.
    pub user: Option<ScopeBackup>,
    pub system: Option<ScopeBackup>,
    /// Variables utilisateur touchées par l'application d'un profil : valeur d'avant
    /// (`None` = n'existait pas, à supprimer à la restauration). Absent des anciennes sauvegardes.
    #[serde(default)]
    pub touched_variables: BTreeMap<String, Option<ScopeBackup>>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct BackupInfo {
    pub file_name: String,
    pub created_at: String,
    pub reason: String,
    pub user_entries: usize,
    pub system_entries: usize,
}

fn count_entries(b: &Option<ScopeBackup>) -> usize {
    b.as_ref().map_or(0, |s| s.raw.split(';').filter(|p| !p.trim().is_empty()).count())
}

/// Refuse un fichier de sauvegarde douteux avant de s'en servir pour écrire.
pub fn validate_backup(b: &Backup) -> Result<(), String> {
    if b.version != BACKUP_VERSION {
        return Err(format!("Version de sauvegarde non prise en charge : {}", b.version));
    }
    let vars = b.touched_variables.iter().map(|(n, s)| (n.as_str(), s));
    let scopes = [("user", &b.user), ("system", &b.system)]
        .into_iter()
        .chain(vars);
    for (name, s) in scopes {
        if let Some(s) = s {
            if s.value_type != "REG_SZ" && s.value_type != "REG_EXPAND_SZ" {
                return Err(format!("Type de valeur invalide pour {name} : {}", s.value_type));
            }
            if s.raw.contains('\0') {
                return Err(format!("Valeur invalide (caractère NUL) pour {name}"));
            }
        }
    }
    Ok(())
}

pub fn encode_reg_string(s: &str) -> Vec<u8> {
    let mut v: Vec<u8> = s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    v.extend_from_slice(&[0, 0]);
    v
}

/// Reconstruit une valeur Registre à partir des entrées conservées.
pub fn join_path(parts: &[&str]) -> String {
    parts.join(";")
}

/// Photo de l'état actuel du Registre (texte brut + type), sans interprétation.
fn snapshot(reason: &str) -> Result<Backup, String> {
    snapshot_with_vars(reason, &[])
}

/// Comme `snapshot`, en gardant aussi la valeur actuelle des variables utilisateur `names`.
pub(crate) fn snapshot_with_vars(reason: &str, names: &[String]) -> Result<Backup, String> {
    let read = |root: HKEY, key: &str| -> Result<Option<ScopeBackup>, String> {
        Ok(read_path_value(root, key)?.map(|(raw, _exp, ty)| ScopeBackup { value_type: ty, raw }))
    };
    let current = if names.is_empty() { BTreeMap::new() } else { read_user_vars()? };
    let touched_variables = names
        .iter()
        .map(|n| {
            let old = current.iter().find(|(k, _)| k.eq_ignore_ascii_case(n)).map(|(_, v)| v.clone());
            (n.clone(), old)
        })
        .collect();
    Ok(Backup {
        version: BACKUP_VERSION,
        created_at: chrono::Local::now().to_rfc3339(),
        reason: reason.to_string(),
        user: read(HKEY_CURRENT_USER, USER_KEY)?,
        system: read(HKEY_LOCAL_MACHINE, SYSTEM_KEY)?,
        touched_variables,
    })
}

// ───────── Variables d'environnement utilisateur (HKCU\Environment) ─────────

/// Toutes les variables utilisateur texte (hors `Path`) : nom → sauvegarde (type + texte).
pub(crate) fn read_user_vars() -> Result<BTreeMap<String, ScopeBackup>, String> {
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(USER_KEY, winreg::enums::KEY_READ)
        .map_err(|e| e.to_string())?;
    let mut out = BTreeMap::new();
    for item in key.enum_values() {
        let (name, v) = item.map_err(|e| e.to_string())?;
        if name.eq_ignore_ascii_case("Path") || name.is_empty() {
            continue;
        }
        let ty = match v.vtype {
            REG_SZ => "REG_SZ",
            REG_EXPAND_SZ => "REG_EXPAND_SZ",
            _ => continue, // DWORD, binaire… : on n'y touche pas
        };
        out.insert(
            name,
            ScopeBackup { value_type: ty.to_string(), raw: crate::decode_reg_string(&v.bytes[..]) },
        );
    }
    Ok(out)
}

fn vtype_of(value_type: &str) -> Result<winreg::enums::RegType, String> {
    match value_type {
        "REG_SZ" => Ok(REG_SZ),
        "REG_EXPAND_SZ" => Ok(REG_EXPAND_SZ),
        other => Err(format!("Type de valeur non pris en charge : {other}")),
    }
}

/// Écrit (ou remplace) une variable utilisateur texte. Jamais `Path` (voir `write_path_value`).
pub(crate) fn write_user_var(name: &str, raw: &str, value_type: &str) -> Result<(), String> {
    if name.eq_ignore_ascii_case("Path") {
        return Err("Path ne s'écrit qu'avec write_path_value".into());
    }
    let vtype = vtype_of(value_type)?;
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(USER_KEY, KEY_SET_VALUE)
        .map_err(|e| format!("Ouverture de HKCU\\Environment impossible : {e}"))?;
    key.set_raw_value(name, &RegValue { bytes: Cow::Owned(encode_reg_string(raw)), vtype })
        .map_err(|e| format!("Écriture de {name} impossible : {e}"))
}

pub(crate) fn delete_user_var(name: &str) -> Result<(), String> {
    if name.eq_ignore_ascii_case("Path") {
        return Err("Suppression de Path refusée".into());
    }
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(USER_KEY, KEY_SET_VALUE)
        .map_err(|e| format!("Ouverture de HKCU\\Environment impossible : {e}"))?;
    match key.delete_value(name) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("Suppression de {name} impossible : {e}")),
        _ => Ok(()),
    }
}

// ───────── Diffusion WM_SETTINGCHANGE ─────────

/// Prévient Windows (explorateur, nouveaux terminaux…) que les variables
/// d'environnement ont changé : `WM_SETTINGCHANGE` avec le paramètre « Environment ».
/// `true` si la diffusion a abouti ; `false` (délai dépassé, fenêtre bloquée) n'est
/// pas une erreur : la modification du Registre, elle, est bien faite.
pub(crate) fn broadcast_environment_change() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
    };
    let param: Vec<u16> = "Environment".encode_utf16().chain(std::iter::once(0)).collect();
    let mut result: usize = 0;
    // SAFETY : `param` est une chaîne UTF-16 terminée par NUL qui vit pendant tout l'appel
    // (SendMessageTimeoutW est synchrone) ; `result` est un pointeur valide.
    let ret = unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            param.as_ptr() as isize,
            SMTO_ABORTIFHUNG,
            3000,
            &mut result,
        )
    };
    ret != 0
}

pub(crate) fn backups_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Dossier de données introuvable : {e}"))?
        .join("backups");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Création de {} impossible : {e}", dir.display()))?;
    Ok(dir)
}

pub(crate) fn write_backup(dir: &Path, b: &Backup) -> Result<PathBuf, String> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S-%3f");
    let file = dir.join(format!("path-backup-{stamp}.json"));
    let json = serde_json::to_string_pretty(b).map_err(|e| e.to_string())?;
    std::fs::write(&file, json).map_err(|e| format!("Écriture de {} impossible : {e}", file.display()))?;
    Ok(file)
}

/// N'accepte qu'un NOM de fichier .json situé dans le dossier de sauvegardes
/// (aucun chemin fourni par l'interface n'est suivi tel quel).
fn resolve_backup_file(dir: &Path, requested: &str) -> Result<PathBuf, String> {
    let name = Path::new(requested)
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("Nom de sauvegarde invalide")?;
    if !name.ends_with(".json") {
        return Err("La sauvegarde doit être un fichier .json".into());
    }
    let p = dir.join(name);
    if !p.is_file() {
        return Err(format!("Sauvegarde introuvable : {name}"));
    }
    Ok(p)
}

fn load_backup(path: &Path) -> Result<Backup, String> {
    let txt = std::fs::read_to_string(path).map_err(|e| format!("Lecture impossible : {e}"))?;
    let b: Backup = serde_json::from_str(&txt).map_err(|e| format!("Sauvegarde illisible : {e}"))?;
    validate_backup(&b)?;
    Ok(b)
}

// ───────────────────────── Écriture Registre ─────────────────────────

pub fn admin_error(what: &str) -> String {
    format!("{ADMIN_REQUIRED}: {what} demande de lancer l'application en tant qu'administrateur.")
}

pub(crate) fn can_write(root: HKEY, subkey: &str) -> bool {
    RegKey::predef(root).open_subkey_with_flags(subkey, KEY_SET_VALUE).is_ok()
}

fn is_denied(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::PermissionDenied || e.raw_os_error() == Some(5)
}

/// Écrit la valeur `Path` en conservant son type (REG_SZ / REG_EXPAND_SZ).
/// Refuse un PATH vide et tout type inconnu.
pub fn write_path_value(root: HKEY, subkey: &str, raw: &str, value_type: &str) -> Result<(), String> {
    let vtype = match value_type {
        "REG_SZ" => REG_SZ,
        "REG_EXPAND_SZ" => REG_EXPAND_SZ,
        other => return Err(format!("Type de valeur non pris en charge : {other}")),
    };
    if raw.trim().is_empty() {
        return Err("Refus d'écrire un PATH vide (utiliser la restauration si besoin).".into());
    }
    let is_system = root == HKEY_LOCAL_MACHINE;
    let key = RegKey::predef(root).open_subkey_with_flags(subkey, KEY_SET_VALUE).map_err(|e| {
        if is_denied(&e) && is_system {
            admin_error("Modifier le PATH Système")
        } else {
            format!("Ouverture de la clé impossible : {e}")
        }
    })?;
    let value = RegValue { bytes: Cow::Owned(encode_reg_string(raw)), vtype };
    key.set_raw_value("Path", &value).map_err(|e| {
        if is_denied(&e) && is_system {
            admin_error("Modifier le PATH Système")
        } else {
            format!("Écriture impossible : {e}")
        }
    })
}

// ───────────────────────── Plans de nettoyage ─────────────────────────

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ScopeSel {
    User,
    System,
    All,
}

impl ScopeSel {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.to_lowercase().as_str() {
            "user" => Ok(Self::User),
            "system" => Ok(Self::System),
            "all" => Ok(Self::All),
            other => Err(format!("Scope inconnu : {other} (attendu : User, System ou All)")),
        }
    }
    fn includes(self, scope: &str) -> bool {
        matches!((self, scope), (Self::All, _) | (Self::User, "User") | (Self::System, "System"))
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Orphans,
    Duplicates,
}

impl Kind {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "orphans" => Ok(Self::Orphans),
            "duplicates" => Ok(Self::Duplicates),
            other => Err(format!("Action inconnue : {other}")),
        }
    }
}

#[derive(Serialize, Debug, PartialEq, Clone)]
pub struct Target {
    pub scope: String,
    /// Position de l'entrée dans la valeur d'origine de son scope.
    pub index: usize,
    pub raw_path: String,
    pub expanded_path: String,
    pub reason: String,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct CleanupPlan {
    pub targets: Vec<Target>,
    /// Entrées introuvables mais volontairement épargnées (variable non résolue, chemin réseau).
    pub skipped_unsafe: usize,
    pub requires_admin: bool,
    pub is_admin: bool,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct CleanupResult {
    pub removed: Vec<Target>,
    pub skipped_unsafe: usize,
    /// Nom du fichier de sauvegarde créé avant l'écriture (None si rien à faire).
    pub backup_file: Option<String>,
    /// WM_SETTINGCHANGE diffusé avec succès (false si rien n'a été écrit ou si la diffusion a expiré).
    pub broadcast: bool,
}

/// Un orphelin n'est supprimé que si on est sûr de son absence : une variable
/// non résolue (`%X%`) ou un chemin réseau injoignable n'est pas une preuve.
pub(crate) fn is_safe_orphan(e: &PathEntry) -> bool {
    !e.expanded_path.contains('%') && !e.expanded_path.trim_start().starts_with(r"\\")
}

pub fn plan_cleanup(report: &RegistryPathReport, kind: Kind, sel: ScopeSel) -> (Vec<Target>, usize) {
    let mut targets = Vec::new();
    let mut skipped = 0;
    for (scope, r) in [("System", &report.system), ("User", &report.user)] {
        if !sel.includes(scope) {
            continue;
        }
        for (index, e) in r.entries.iter().enumerate() {
            let reason = match kind {
                Kind::Orphans if !e.exists => {
                    if is_safe_orphan(e) {
                        "Orphelin (dossier introuvable)"
                    } else {
                        skipped += 1;
                        continue;
                    }
                }
                Kind::Duplicates if e.is_duplicate => "Doublon",
                _ => continue,
            };
            targets.push(Target {
                scope: scope.to_string(),
                index,
                raw_path: e.raw_path.clone(),
                expanded_path: e.expanded_path.clone(),
                reason: reason.to_string(),
            });
        }
    }
    (targets, skipped)
}

/// Nouvelle valeur d'un scope : toutes les entrées sauf les cibles, jointes par `;`.
pub fn new_scope_value(entries: &[PathEntry], scope: &str, targets: &[Target]) -> String {
    let keep: Vec<&str> = entries
        .iter()
        .enumerate()
        .filter(|(i, _)| !targets.iter().any(|t| t.scope == scope && t.index == *i))
        .map(|(_, e)| e.raw_path.as_str())
        .collect();
    join_path(&keep)
}

fn run_cleanup(dir: &Path, kind: Kind, sel: ScopeSel, reason: &str) -> Result<CleanupResult, String> {
    let report = build_report();
    for (name, r) in [("User", &report.user), ("System", &report.system)] {
        if let (true, Some(e)) = (sel.includes(name), &r.error) {
            return Err(format!("Lecture du PATH {name} impossible : {e}"));
        }
    }
    let (targets, skipped) = plan_cleanup(&report, kind, sel);
    if targets.is_empty() {
        return Ok(CleanupResult { removed: vec![], skipped_unsafe: skipped, backup_file: None, broadcast: false });
    }
    let touches = |s: &str| targets.iter().any(|t| t.scope == s);

    // Contrôle des droits AVANT toute écriture : pas de modification partielle.
    if touches("System") && !can_write(HKEY_LOCAL_MACHINE, SYSTEM_KEY) {
        return Err(admin_error("Modifier le PATH Système"));
    }
    if touches("User") && !can_write(HKEY_CURRENT_USER, USER_KEY) {
        return Err("Écriture refusée sur HKCU\\Environment.".into());
    }

    let file = write_backup(dir, &snapshot(reason)?)?;
    let file_name = file.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();

    if touches("System") {
        let ty = report.system.value_type.as_deref().unwrap_or_default();
        write_path_value(HKEY_LOCAL_MACHINE, SYSTEM_KEY, &new_scope_value(&report.system.entries, "System", &targets), ty)
            .map_err(|e| format!("{e} (sauvegarde : {file_name})"))?;
    }
    if touches("User") {
        let ty = report.user.value_type.as_deref().unwrap_or_default();
        write_path_value(HKEY_CURRENT_USER, USER_KEY, &new_scope_value(&report.user.entries, "User", &targets), ty)
            .map_err(|e| format!("{e} (sauvegarde : {file_name})"))?;
    }
    // Écriture réussie : prévenir Windows pour que les nouveaux processus voient le changement.
    let broadcast = broadcast_environment_change();
    Ok(CleanupResult { removed: targets, skipped_unsafe: skipped, backup_file: Some(file_name), broadcast })
}

// ───────────────────────── Commandes Tauri ─────────────────────────

/// Vrai si le processus peut écrire dans HKLM\...\Environment (donc admin).
#[tauri::command]
pub async fn is_admin() -> bool {
    can_write(HKEY_LOCAL_MACHINE, SYSTEM_KEY)
}

/// Sauvegarde le PATH HKCU et HKLM dans `backups/` ; renvoie le chemin du fichier.
#[tauri::command]
pub async fn create_registry_backup(app: tauri::AppHandle) -> Result<String, String> {
    let dir = backups_dir(&app)?;
    let file = write_backup(&dir, &snapshot("manuelle")?)?;
    Ok(file.display().to_string())
}

/// Restaure une sauvegarde (par nom de fichier). Une sauvegarde de l'état actuel
/// est créée juste avant, pour pouvoir annuler la restauration.
#[tauri::command]
pub async fn restore_registry_backup(app: tauri::AppHandle, backup_file: String) -> Result<bool, String> {
    let dir = backups_dir(&app)?;
    let b = load_backup(&resolve_backup_file(&dir, &backup_file)?)?;
    let var_names: Vec<String> = b.touched_variables.keys().cloned().collect();
    let current = snapshot_with_vars("avant-restauration", &var_names)?;

    let changes = |new: &Option<ScopeBackup>, old: &Option<ScopeBackup>| new.is_some() && new != old;
    let do_system = changes(&b.system, &current.system);
    let do_user = changes(&b.user, &current.user);
    // Variables touchées par un profil : à remettre comme avant (ou à supprimer si elles n'existaient pas).
    let var_changes: Vec<(&String, &Option<ScopeBackup>)> =
        b.touched_variables.iter().filter(|(n, old)| current.touched_variables.get(*n) != Some(*old)).collect();
    if do_system && !can_write(HKEY_LOCAL_MACHINE, SYSTEM_KEY) {
        return Err(admin_error("Restaurer le PATH Système"));
    }
    if !do_system && !do_user && var_changes.is_empty() {
        return Ok(false);
    }
    write_backup(&dir, &current)?;
    if let (true, Some(s)) = (do_system, &b.system) {
        write_path_value(HKEY_LOCAL_MACHINE, SYSTEM_KEY, &s.raw, &s.value_type)?;
    }
    if let (true, Some(s)) = (do_user, &b.user) {
        write_path_value(HKEY_CURRENT_USER, USER_KEY, &s.raw, &s.value_type)?;
    }
    for (name, old) in var_changes {
        match old {
            Some(s) => write_user_var(name, &s.raw, &s.value_type)?,
            None => delete_user_var(name)?,
        }
    }
    Ok(broadcast_environment_change())
}

#[derive(Serialize, Debug, PartialEq)]
pub struct VarDiff {
    pub name: String,
    pub current: Option<String>,
    pub backup: Option<String>,
}

/// Ce que changerait la restauration d'une sauvegarde (rien n'est écrit).
#[derive(Serialize, Debug, PartialEq)]
pub struct RestorePreview {
    pub created_at: String,
    pub reason: String,
    /// `Some` seulement si la restauration modifierait ce scope.
    pub user: Option<ScopeDiff>,
    pub system: Option<ScopeDiff>,
    pub variables: Vec<VarDiff>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct ScopeDiff {
    pub current: Vec<String>,
    pub backup: Vec<String>,
}

fn split_entries(raw: &str) -> Vec<String> {
    raw.split(';').map(str::trim).filter(|p| !p.is_empty()).map(String::from).collect()
}

/// Même logique de décision que `restore_registry_backup`, sans écrire.
pub fn build_restore_preview(b: &Backup, current: &Backup) -> RestorePreview {
    let diff = |new: &Option<ScopeBackup>, old: &Option<ScopeBackup>| match (new, old) {
        (Some(n), o) if Some(n) != o.as_ref() => Some(ScopeDiff {
            current: o.as_ref().map(|s| split_entries(&s.raw)).unwrap_or_default(),
            backup: split_entries(&n.raw),
        }),
        _ => None,
    };
    let variables = b
        .touched_variables
        .iter()
        .filter(|(n, old)| current.touched_variables.get(*n) != Some(*old))
        .map(|(n, old)| VarDiff {
            name: n.clone(),
            current: current.touched_variables.get(n).and_then(|v| v.as_ref()).map(|s| s.raw.clone()),
            backup: old.as_ref().map(|s| s.raw.clone()),
        })
        .collect();
    RestorePreview {
        created_at: b.created_at.clone(),
        reason: b.reason.clone(),
        user: diff(&b.user, &current.user),
        system: diff(&b.system, &current.system),
        variables,
    }
}

/// Aperçu (lecture seule) d'une restauration, pour la modale « diff ».
#[tauri::command]
pub async fn preview_restore(app: tauri::AppHandle, backup_file: String) -> Result<RestorePreview, String> {
    let dir = backups_dir(&app)?;
    let b = load_backup(&resolve_backup_file(&dir, &backup_file)?)?;
    let names: Vec<String> = b.touched_variables.keys().cloned().collect();
    let current = snapshot_with_vars("aperçu", &names)?;
    Ok(build_restore_preview(&b, &current))
}

/// Les 10 sauvegardes les plus récentes.
#[tauri::command]
pub async fn list_backups(app: tauri::AppHandle) -> Result<Vec<BackupInfo>, String> {
    let dir = backups_dir(&app)?;
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.starts_with("path-backup-") && n.ends_with(".json"))
        .collect();
    names.sort_unstable_by(|a, b| b.cmp(a)); // l'horodatage dans le nom trie par date
    Ok(names
        .into_iter()
        .take(10)
        .filter_map(|n| {
            let b = load_backup(&dir.join(&n)).ok()?;
            Some(BackupInfo {
                file_name: n,
                created_at: b.created_at.clone(),
                reason: b.reason.clone(),
                user_entries: count_entries(&b.user),
                system_entries: count_entries(&b.system),
            })
        })
        .collect())
}

/// Aperçu (lecture seule) de ce que supprimerait `kind` ("orphans" | "duplicates").
#[tauri::command]
pub async fn preview_cleanup(kind: String, scope: String) -> Result<CleanupPlan, String> {
    let (kind, sel) = (Kind::parse(&kind)?, ScopeSel::parse(&scope)?);
    let (targets, skipped_unsafe) = plan_cleanup(&build_report(), kind, sel);
    let requires_admin = targets.iter().any(|t| t.scope == "System");
    Ok(CleanupPlan { targets, skipped_unsafe, requires_admin, is_admin: can_write(HKEY_LOCAL_MACHINE, SYSTEM_KEY) })
}

/// Supprime du Registre les entrées dont le dossier n'existe plus.
#[tauri::command]
pub async fn remove_orphans(app: tauri::AppHandle, scope: String) -> Result<CleanupResult, String> {
    run_cleanup(&backups_dir(&app)?, Kind::Orphans, ScopeSel::parse(&scope)?, "avant-suppression-orphelins")
}

/// Garde la première occurrence de chaque chemin et supprime les suivantes.
#[tauri::command]
pub async fn deduplicate_paths(app: tauri::AppHandle, scope: String) -> Result<CleanupResult, String> {
    run_cleanup(&backups_dir(&app)?, Kind::Duplicates, ScopeSel::parse(&scope)?, "avant-dedoublonnage")
}

// ───────────────────────── Tests ─────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{analyze_registry_path, decode_reg_string, ScopeReport};
    use std::collections::HashSet;

    fn report(user: &str, system: &str, exists: impl Fn(&str) -> bool) -> RegistryPathReport {
        let lookup = |n: &str| (n.to_lowercase() == "root").then(|| r"C:\R".to_string());
        let mut seen = HashSet::new();
        let sys = analyze_registry_path("System", system, true, &lookup, &exists, &mut seen);
        let usr = analyze_registry_path("User", user, true, &lookup, &exists, &mut seen);
        let sr = |entries| ScopeReport { entries, value_type: Some("REG_EXPAND_SZ".into()), error: None };
        RegistryPathReport { user: sr(usr), system: sr(sys) }
    }

    #[test]
    fn backup_serialisation_aller_retour() {
        let b = Backup {
            version: 1,
            created_at: "2026-11-09T10:00:00+01:00".into(),
            reason: "manuelle".into(),
            user: Some(ScopeBackup { value_type: "REG_EXPAND_SZ".into(), raw: r"%USERPROFILE%\bin;C:\Café".into() }),
            system: None,
            touched_variables: BTreeMap::new(),
        };
        let json = serde_json::to_string_pretty(&b).unwrap();
        assert!(json.contains("\"value_type\": \"REG_EXPAND_SZ\""));
        assert!(json.contains("\"system\": null"));
        assert_eq!(serde_json::from_str::<Backup>(&json).unwrap(), b);
        assert!(validate_backup(&b).is_ok());
    }

    #[test]
    fn backup_invalide_refuse() {
        let ok = ScopeBackup { value_type: "REG_SZ".into(), raw: "C:\\a".into() };
        let mk = |v: u32, s: ScopeBackup| Backup { version: v, created_at: String::new(), reason: String::new(), user: Some(s), system: None, touched_variables: BTreeMap::new() };
        assert!(validate_backup(&mk(2, ok.clone())).is_err());
        assert!(validate_backup(&mk(1, ScopeBackup { value_type: "REG_BINARY".into(), raw: "x".into() })).is_err());
        assert!(validate_backup(&mk(1, ScopeBackup { value_type: "REG_SZ".into(), raw: "a\0b".into() })).is_err());
        assert!(serde_json::from_str::<Backup>("{\"version\":1}").is_err());
    }

    #[test]
    fn jointure_par_point_virgule() {
        assert_eq!(join_path(&[r"C:\a", r"%X%\b", r"D:\c"]), r"C:\a;%X%\b;D:\c");
        assert_eq!(join_path(&[]), "");
        assert_eq!(join_path(&["seul"]), "seul");
    }

    #[test]
    fn encodage_utf16_aller_retour() {
        let s = r"C:\Café;%X%\ü";
        let b = encode_reg_string(s);
        assert_eq!(&b[b.len() - 2..], &[0, 0]);
        assert_eq!(decode_reg_string(&b), s);
    }

    #[test]
    fn nom_de_sauvegarde_ne_suit_aucun_chemin() {
        let dir = std::env::temp_dir().join(format!("dd-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.json"), "{}").unwrap();
        assert!(resolve_backup_file(&dir, "a.json").is_ok());
        // Un chemin est réduit à son nom : on ne sort pas du dossier.
        assert!(resolve_backup_file(&dir, r"..\..\Windows\win.ini").is_err());
        assert!(resolve_backup_file(&dir, r"C:\autre\a.json").is_ok()); // = dir\a.json
        assert!(resolve_backup_file(&dir, "a.txt").is_err());
        assert!(resolve_backup_file(&dir, "absent.json").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scope_et_action_parses() {
        assert_eq!(ScopeSel::parse("user").unwrap(), ScopeSel::User);
        assert_eq!(ScopeSel::parse("System").unwrap(), ScopeSel::System);
        assert_eq!(ScopeSel::parse("ALL").unwrap(), ScopeSel::All);
        assert!(ScopeSel::parse("autre").is_err());
        assert!(Kind::parse("orphans").is_ok() && Kind::parse("x").is_err());
    }

    #[test]
    fn orphelins_surs_seulement() {
        let ex = |p: &str| p == r"C:\Ok";
        let r = report(r"C:\Ok;C:\Absent;%INCONNUE%\x;\\srv\partage;%ROOT%\absent", "", ex);
        let (t, skipped) = plan_cleanup(&r, Kind::Orphans, ScopeSel::All);
        let raws: Vec<&str> = t.iter().map(|x| x.raw_path.as_str()).collect();
        assert_eq!(raws, vec![r"C:\Absent", r"%ROOT%\absent"]);
        assert_eq!(skipped, 2); // variable non résolue + chemin réseau
    }

    #[test]
    fn scope_limite_le_plan() {
        let ex = |_: &str| false;
        let r = report(r"C:\U", r"C:\S", ex);
        let count = |s| plan_cleanup(&r, Kind::Orphans, s).0.len();
        assert_eq!((count(ScopeSel::User), count(ScopeSel::System), count(ScopeSel::All)), (1, 1, 2));
    }

    #[test]
    fn dedoublonnage_garde_la_premiere_occurrence_systeme_d_abord() {
        let r = report(r"C:\Bin;c:\bin\;C:\Sys;C:\U", r"C:\Sys;C:\Bin", |_| true);
        // Système : rien. Utilisateur : C:\Bin (déjà système), c:\bin\ et C:\Sys sont des doublons.
        let (t, _) = plan_cleanup(&r, Kind::Duplicates, ScopeSel::All);
        let got: Vec<(&str, usize)> = t.iter().map(|x| (x.scope.as_str(), x.index)).collect();
        assert_eq!(got, vec![("User", 0), ("User", 1), ("User", 2)]);
        // Limité au scope Système : aucun doublon interne.
        assert!(plan_cleanup(&r, Kind::Duplicates, ScopeSel::System).0.is_empty());
        let nv = new_scope_value(&r.user.entries, "User", &t);
        assert_eq!(nv, r"C:\U");
    }

    #[test]
    fn reconstruction_garde_le_brut_et_retire_par_index() {
        // Deux entrées au texte identique : seule la 2e (doublon) part.
        let r = report(r"%ROOT%\a;%ROOT%\a;D:\z", "", |_| true);
        let (t, _) = plan_cleanup(&r, Kind::Duplicates, ScopeSel::User);
        assert_eq!(t.len(), 1);
        assert_eq!(new_scope_value(&r.user.entries, "User", &t), r"%ROOT%\a;D:\z");
    }

    #[test]
    fn ecriture_conserve_le_type_et_refuse_le_dangereux() {
        // Clé de test isolée dans HKCU, supprimée à la fin (aucune clé réelle touchée).
        let sub = format!(r"Software\DevEnvDoctorTest-{}", std::process::id());
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        hkcu.create_subkey(&sub).unwrap();
        let cleanup = || { let _ = hkcu.delete_subkey_all(&sub); };

        let res = std::panic::catch_unwind(|| {
            write_path_value(HKEY_CURRENT_USER, &sub, r"%USERPROFILE%\b;C:\é", "REG_EXPAND_SZ").unwrap();
            let (raw, expandable, ty) = read_path_value(HKEY_CURRENT_USER, &sub).unwrap().unwrap();
            assert_eq!((raw.as_str(), expandable, ty.as_str()), (r"%USERPROFILE%\b;C:\é", true, "REG_EXPAND_SZ"));

            write_path_value(HKEY_CURRENT_USER, &sub, r"C:\a;C:\b", "REG_SZ").unwrap();
            let (raw, expandable, ty) = read_path_value(HKEY_CURRENT_USER, &sub).unwrap().unwrap();
            assert_eq!((raw.as_str(), expandable, ty.as_str()), (r"C:\a;C:\b", false, "REG_SZ"));

            assert!(write_path_value(HKEY_CURRENT_USER, &sub, "  ", "REG_SZ").is_err());
            assert!(write_path_value(HKEY_CURRENT_USER, &sub, "x", "Autre").is_err());
            // Les refus n'ont rien modifié.
            assert_eq!(read_path_value(HKEY_CURRENT_USER, &sub).unwrap().unwrap().0, r"C:\a;C:\b");
        });
        cleanup();
        assert!(res.is_ok());
    }

    #[test]
    fn erreur_admin_a_un_prefixe_stable() {
        assert!(admin_error("Modifier le PATH Système").starts_with("AdminRequired: "));
    }

    #[test]
    fn ancienne_sauvegarde_sans_variables_reste_lisible_et_variables_validees() {
        let old = r#"{"version":1,"created_at":"t","reason":"manuelle","user":null,"system":null}"#;
        let b: Backup = serde_json::from_str(old).unwrap();
        assert!(b.touched_variables.is_empty() && validate_backup(&b).is_ok());

        let mut b2 = b.clone();
        b2.touched_variables.insert("EDITOR".into(), Some(ScopeBackup { value_type: "REG_SZ".into(), raw: "vim".into() }));
        b2.touched_variables.insert("NOUVELLE".into(), None);
        let back: Backup = serde_json::from_str(&serde_json::to_string(&b2).unwrap()).unwrap();
        assert_eq!(back, b2);
        assert!(validate_backup(&b2).is_ok());
        b2.touched_variables.insert("MAUVAISE".into(), Some(ScopeBackup { value_type: "REG_DWORD".into(), raw: "1".into() }));
        assert!(validate_backup(&b2).is_err());
    }

    #[test]
    fn apercu_de_restauration_ne_liste_que_ce_qui_change() {
        let sb = |raw: &str| Some(ScopeBackup { value_type: "REG_EXPAND_SZ".into(), raw: raw.into() });
        let mk = |u: Option<ScopeBackup>, s: Option<ScopeBackup>, v: &[(&str, Option<&str>)]| Backup {
            version: 1, created_at: "t".into(), reason: "r".into(), user: u, system: s,
            touched_variables: v.iter().map(|(n, x)| (n.to_string(), x.map(|r| ScopeBackup { value_type: "REG_SZ".into(), raw: r.into() }))).collect(),
        };
        let backup = mk(sb(r"C:;C:"), sb(r"C:s"), &[("X", Some("old")), ("Y", None), ("Z", Some("same"))]);
        let current = mk(sb(r"C:"), sb(r"C:s"), &[("X", Some("new")), ("Y", Some("created")), ("Z", Some("same"))]);
        let p = build_restore_preview(&backup, &current);
        let u = p.user.unwrap();
        assert_eq!((u.current, u.backup), (vec![r"C:".to_string()], vec![r"C:".to_string(), r"C:".to_string()]));
        assert!(p.system.is_none()); // identique : rien à restaurer
        let names: Vec<&str> = p.variables.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, vec!["X", "Y"]);
        assert_eq!(p.variables[1].backup, None); // Y n'existait pas : sera supprimée
        // Valeur absente dans la sauvegarde : le scope n'est jamais touché.
        assert!(build_restore_preview(&mk(None, None, &[]), &current).user.is_none());
    }
}
