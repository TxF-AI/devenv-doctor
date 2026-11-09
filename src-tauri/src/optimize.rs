//! Optimisations du PATH : longueur et risque, compression, réordonnancement,
//! conflits de binaires. Même discipline que `actions` : aperçu en lecture seule,
//! sauvegarde automatique avant écriture, droits vérifiés avant la 1re écriture.

use crate::actions::{
    admin_error, backups_dir, broadcast_environment_change, can_reuse_backup, can_write, latest_backup,
    snapshot_with_vars, write_backup, write_path_value, ScopeSel,
};
use crate::{
    build_report, dedup_key, expand_env, read_path_value, RegistryPathReport, SYSTEM_KEY, USER_KEY,
};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

// ───────────────────────── Longueur et risque ─────────────────────────

/// Seuils : `setx` tronque à 1024 ; 2047 = limite historique de certains outils ;
/// 8191 = longueur maximale d'une ligne de commande `cmd`.
pub const THRESHOLDS: [usize; 3] = [1024, 2048, 8192];

#[derive(Serialize, Debug, PartialEq, Clone)]
pub struct Risk {
    /// "ok" | "near" | "medium" | "high" | "critical"
    pub level: &'static str,
    pub message: String,
}

pub fn assess(len: usize) -> Risk {
    let (level, message) = if len >= THRESHOLDS[2] {
        ("critical", tr!("Critique : dépasse 8191 caractères, limite d'une ligne de commande cmd.", "Critical: exceeds 8191 characters, the cmd command-line limit.").to_string())
    } else if len >= THRESHOLDS[1] {
        ("high", tr!("Élevé : dépasse 2047 caractères, limite historique de certains outils.", "High: exceeds 2047 characters, the historical limit of some tools.").to_string())
    } else if len >= THRESHOLDS[0] {
        ("medium", tr!("Attention : `setx` tronque le PATH au-delà de 1024 caractères.", "Warning: `setx` truncates PATH beyond 1024 characters.").to_string())
    } else if len * 100 >= THRESHOLDS[0] * 85 {
        ("near", tr!(format!("Proche du seuil `setx` (1024) : {} caractères de marge.", THRESHOLDS[0] - len), format!("Close to the `setx` limit (1024): {} characters of margin.", THRESHOLDS[0] - len)))
    } else {
        ("ok", "OK".to_string())
    };
    Risk { level, message }
}

/// Longueur Windows d'une chaîne = nombre d'unités UTF-16.
pub fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

#[derive(Serialize, Debug, PartialEq)]
pub struct LengthInfo {
    pub length: usize,
    pub risk: Risk,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct LengthReport {
    /// Valeur HKCU brute (non développée).
    pub user: LengthInfo,
    /// Valeur HKLM brute (non développée).
    pub system: LengthInfo,
    /// PATH effectif d'un processus : Système + « ; » + Utilisateur, variables développées.
    pub combined: LengthInfo,
}

fn info(len: usize) -> LengthInfo {
    LengthInfo { length: len, risk: assess(len) }
}

/// `(texte brut, REG_EXPAND_SZ ?)` de chaque scope ; `None` si absent.
pub fn build_length_report(
    user: Option<(&str, bool)>,
    system: Option<(&str, bool)>,
    lookup: &impl Fn(&str) -> Option<String>,
) -> LengthReport {
    let expand = |v: Option<(&str, bool)>| match v {
        Some((raw, true)) => expand_env(raw, lookup),
        Some((raw, false)) => raw.to_string(),
        None => String::new(),
    };
    let (u, s) = (expand(user), expand(system));
    let combined = match (u.is_empty(), s.is_empty()) {
        (true, true) => 0,
        (false, true) => utf16_len(&u),
        (true, false) => utf16_len(&s),
        (false, false) => utf16_len(&s) + 1 + utf16_len(&u),
    };
    LengthReport {
        user: info(user.map_or(0, |(r, _)| utf16_len(r))),
        system: info(system.map_or(0, |(r, _)| utf16_len(r))),
        combined: info(combined),
    }
}

#[tauri::command]
pub async fn get_path_lengths() -> Result<LengthReport, String> {
    let user = read_path_value(HKEY_CURRENT_USER, USER_KEY)?;
    let system = read_path_value(HKEY_LOCAL_MACHINE, SYSTEM_KEY)?;
    Ok(build_length_report(
        user.as_ref().map(|(r, e, _)| (r.as_str(), *e)),
        system.as_ref().map(|(r, e, _)| (r.as_str(), *e)),
        &|n: &str| std::env::var(n).ok(),
    ))
}

// ───────────────────────── Écriture commune ─────────────────────────

struct ScopeEdit {
    scope: &'static str,
    new_raw: String,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct WriteOutcome {
    pub backup_file: Option<String>,
    pub broadcast: bool,
}

/// Écrit les nouvelles valeurs : droits vérifiés AVANT toute écriture, sauvegarde
/// (réutilisée si `reuse_window` et récente), type conservé (REG_SZ passe en
/// REG_EXPAND_SZ si la valeur contient un `%VAR%`), puis diffusion WM_SETTINGCHANGE.
fn commit_scope_edits(
    dir: &Path,
    reason: &str,
    reuse_window: Option<i64>,
    report: &RegistryPathReport,
    edits: &[ScopeEdit],
) -> Result<WriteOutcome, String> {
    if edits.is_empty() {
        return Ok(WriteOutcome { backup_file: None, broadcast: false });
    }
    let touches = |s: &str| edits.iter().any(|e| e.scope == s);
    if touches("System") && !can_write(HKEY_LOCAL_MACHINE, SYSTEM_KEY) {
        return Err(admin_error(tr!("Modifier le PATH Système", "Modify the System PATH")));
    }
    if touches("User") && !can_write(HKEY_CURRENT_USER, USER_KEY) {
        return Err(tr!("Écriture refusée sur HKCU\\Environment.", "Write refused on HKCU\\Environment.").into());
    }

    let reused = reuse_window.and_then(|w| {
        let (name, b) = latest_backup(dir)?;
        let age = chrono::DateTime::parse_from_rfc3339(&b.created_at)
            .ok()
            .map(|t| chrono::Local::now().timestamp() - t.timestamp());
        can_reuse_backup(&b.reason, age, reason, w).then_some(name)
    });
    let file_name = match reused {
        Some(n) => n,
        None => {
            let f = write_backup(dir, &snapshot_with_vars(reason, &[])?)?;
            f.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string()
        }
    };

    for edit in edits {
        let (root, key, current) = if edit.scope == "System" {
            (HKEY_LOCAL_MACHINE, SYSTEM_KEY, &report.system)
        } else {
            (HKEY_CURRENT_USER, USER_KEY, &report.user)
        };
        let ty = match current.value_type.as_deref() {
            Some("REG_SZ") if !edit.new_raw.contains('%') => "REG_SZ",
            Some("REG_SZ") | Some("REG_EXPAND_SZ") => "REG_EXPAND_SZ",
            other => return Err(tr!(format!("Type de valeur non pris en charge : {other:?}"), format!("Unsupported value type: {other:?}"))),
        };
        write_path_value(root, key, &edit.new_raw, ty).map_err(|e| format!("{e} ({}: {file_name})", tr!("sauvegarde", "backup")))?;
    }
    Ok(WriteOutcome { backup_file: Some(file_name), broadcast: broadcast_environment_change() })
}

fn scope_entries<'a>(report: &'a RegistryPathReport, scope: &str) -> &'a crate::ScopeReport {
    if scope == "System" { &report.system } else { &report.user }
}

fn join_raw(entries: impl IntoIterator<Item = String>) -> String {
    entries.into_iter().collect::<Vec<_>>().join(";")
}

// ───────────────────────── Compression ─────────────────────────

fn starts_with_dir(s: &str, dir: &str) -> bool {
    s.len() >= dir.len()
        && s.is_char_boundary(dir.len())
        && s[..dir.len()].eq_ignore_ascii_case(dir)
        && matches!(s[dir.len()..].chars().next(), None | Some('\\'))
}

/// Un remplacement possible : un dossier réel → la variable qui le désigne.
#[derive(Debug, Clone, PartialEq)]
pub struct Substitution {
    pub token: &'static str,
    pub dir: String,
    /// Dossier propre à l'utilisateur : interdit dans le PATH Système, où la même
    /// variable changerait de sens selon la session.
    pub user_only: bool,
}

/// Remplacements, du préfixe le plus long au plus court (le premier qui raccourcit gagne).
/// `%windir%` suit `%SystemRoot%` : même dossier, 4 caractères de moins.
pub fn substitutions(lookup: &impl Fn(&str) -> Option<String>) -> Vec<Substitution> {
    [
        ("%LOCALAPPDATA%", "LOCALAPPDATA", true),
        ("%APPDATA%", "APPDATA", true),
        ("%USERPROFILE%", "USERPROFILE", true),
        ("%SystemRoot%", "SystemRoot", false),
        ("%windir%", "SystemRoot", false),
    ]
    .into_iter()
    .filter_map(|(token, var, user_only)| {
        lookup(var).map(|dir| Substitution { token, dir: dir.trim_end_matches('\\').to_string(), user_only })
    })
    .collect()
}

/// Remplace un préfixe de dossier ENTIER par sa variable, mais seulement si l'entrée
/// devient plus courte : `C:\Windows` → `%SystemRoot%` rallongerait, on prend `%windir%`.
/// Les entrées déjà variabilisées ou entre guillemets ne sont pas touchées.
pub fn compress_entry(raw: &str, scope: &str, subs: &[Substitution]) -> String {
    let t = raw.trim();
    if t.contains('%') || t.starts_with('"') {
        return t.to_string();
    }
    for s in subs {
        if (s.user_only && scope != "User") || s.dir.len() <= 3 || !starts_with_dir(t, &s.dir) {
            continue;
        }
        let candidate = format!("{}{}", s.token, &t[s.dir.len()..]);
        if utf16_len(&candidate) < utf16_len(t) {
            return candidate;
        }
    }
    t.to_string()
}

#[derive(Serialize, Debug, PartialEq, Clone)]
pub struct CompressChange {
    pub scope: String,
    pub index: usize,
    pub old: String,
    pub new: String,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct CompressPlan {
    pub changes: Vec<CompressChange>,
    pub saved_chars: usize,
    pub requires_admin: bool,
    pub is_admin: bool,
}

pub fn plan_compress(report: &RegistryPathReport, sel: ScopeSel, subs: &[Substitution]) -> Vec<CompressChange> {
    let mut out = Vec::new();
    for scope in ["System", "User"] {
        if !sel.includes(scope) {
            continue;
        }
        for (index, e) in scope_entries(report, scope).entries.iter().enumerate() {
            let new = compress_entry(&e.raw_path, scope, subs);
            if new != e.raw_path {
                out.push(CompressChange { scope: scope.to_string(), index, old: e.raw_path.clone(), new });
            }
        }
    }
    out
}

fn saved(changes: &[CompressChange]) -> usize {
    changes.iter().map(|c| utf16_len(&c.old).saturating_sub(utf16_len(&c.new))).sum()
}

fn env_subs() -> Vec<Substitution> {
    substitutions(&|n: &str| std::env::var(n).ok())
}

/// Aperçu (lecture seule) de la compression.
#[tauri::command]
pub async fn preview_compress(scope: String) -> Result<CompressPlan, String> {
    let sel = ScopeSel::parse(&scope)?;
    let changes = plan_compress(&build_report(), sel, &env_subs());
    Ok(CompressPlan {
        saved_chars: saved(&changes),
        requires_admin: changes.iter().any(|c| c.scope == "System"),
        is_admin: can_write(HKEY_LOCAL_MACHINE, SYSTEM_KEY),
        changes,
    })
}

#[derive(Serialize, Debug, PartialEq)]
pub struct CompressResult {
    pub changes: Vec<CompressChange>,
    pub saved_chars: usize,
    pub backup_file: Option<String>,
    pub broadcast: bool,
}

/// Compresse le PATH : `C:\Users\<nom>` → `%USERPROFILE%`, dossier Windows → `%SystemRoot%`.
#[tauri::command]
pub async fn compress_path(app: tauri::AppHandle, scope: String) -> Result<CompressResult, String> {
    let sel = ScopeSel::parse(&scope)?;
    let report = build_report();
    for (name, r) in [("User", &report.user), ("System", &report.system)] {
        if let (true, Some(e)) = (sel.includes(name), &r.error) {
            return Err(tr!(format!("Lecture du PATH {name} impossible : {e}"), format!("Cannot read the {name} PATH: {e}")));
        }
    }
    let changes = plan_compress(&report, sel, &env_subs());
    let mut edits = Vec::new();
    for scope in ["System", "User"] {
        if changes.iter().any(|c| c.scope == scope) {
            let raw = join_raw(scope_entries(&report, scope).entries.iter().enumerate().map(|(i, e)| {
                changes.iter().find(|c| c.scope == scope && c.index == i).map_or(e.raw_path.clone(), |c| c.new.clone())
            }));
            edits.push(ScopeEdit { scope: if scope == "System" { "System" } else { "User" }, new_raw: raw });
        }
    }
    let out = commit_scope_edits(&backups_dir(&app)?, "avant-compression", None, &report, &edits)?;
    Ok(CompressResult { saved_chars: saved(&changes), changes, backup_file: out.backup_file, broadcast: out.broadcast })
}

// ───────────────────────── Réordonnancement ─────────────────────────

/// Échange l'entrée `index` avec sa voisine ("up" = vers le début = plus prioritaire).
pub fn swap_entries(entries: &[String], index: usize, direction: &str) -> Result<Vec<String>, String> {
    if index >= entries.len() {
        return Err(tr!(format!("Position {index} hors limites ({} entrées).", entries.len()), format!("Position {index} out of range ({} entries).", entries.len())));
    }
    let other = match direction {
        "up" if index > 0 => index - 1,
        "up" => return Err(tr!("Cette entrée est déjà la première.", "This entry is already the first one.").into()),
        "down" if index + 1 < entries.len() => index + 1,
        "down" => return Err(tr!("Cette entrée est déjà la dernière.", "This entry is already the last one.").into()),
        other => return Err(tr!(format!("Direction inconnue : {other} (attendu : up ou down)"), format!("Unknown direction: {other} (expected: up or down)"))),
    };
    let mut v = entries.to_vec();
    v.swap(index, other);
    Ok(v)
}

/// Monte ou descend une entrée DANS son scope (Système passe toujours avant Utilisateur).
/// `expected_path` est le texte vu par l'interface : refus si le PATH a changé entre-temps.
#[tauri::command]
pub async fn move_path_entry(
    app: tauri::AppHandle,
    scope: String,
    index: usize,
    direction: String,
    expected_path: String,
) -> Result<WriteOutcome, String> {
    let scope: &'static str = match ScopeSel::parse(&scope)? {
        ScopeSel::User => "User",
        ScopeSel::System => "System",
        ScopeSel::All => return Err(tr!("Réordonner demande un scope précis : User ou System.", "Reordering needs a specific scope: User or System.").into()),
    };
    let report = build_report();
    let r = scope_entries(&report, scope);
    if let Some(e) = &r.error {
        return Err(tr!(format!("Lecture du PATH impossible : {e}"), format!("Cannot read the PATH: {e}")));
    }
    let raws: Vec<String> = r.entries.iter().map(|e| e.raw_path.clone()).collect();
    if raws.get(index).map(String::as_str) != Some(expected_path.as_str()) {
        return Err(tr!("Le PATH a changé depuis l'affichage : recharge la liste.", "The PATH changed since it was displayed: reload the list.").into());
    }
    let new = swap_entries(&raws, index, &direction)?;
    // 5 min : une série de déplacements ne crée qu'UNE sauvegarde (l'état d'avant la série).
    commit_scope_edits(&backups_dir(&app)?, "avant-reordonnancement", Some(300), &report, &[ScopeEdit { scope, new_raw: join_raw(new) }])
}

// ───────────────────────── Conflits de binaires ─────────────────────────

/// Programmes d'installation et utilitaires génériques présents partout : bruit, pas conflit.
fn is_noise(name: &str) -> bool {
    ["unins", "uninstall", "setup", "update", "install", "vc_redist", "vcredist"]
        .iter()
        .any(|p| name.starts_with(p))
}

fn is_executable(name: &str) -> bool {
    matches!(name.rsplit_once('.'), Some((_, "exe" | "cmd" | "bat" | "com")))
}

#[derive(Serialize, Debug, PartialEq, Clone)]
pub struct ConflictName {
    pub name: String,
    /// Cette entrée est celle que Windows lance (1re dans l'ordre de résolution).
    pub wins: bool,
    /// Une autre entrée qui contient le même binaire (la gagnante si `wins` est faux).
    pub other_scope: String,
    pub other_index: usize,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct EntryConflicts {
    pub scope: String,
    pub index: usize,
    /// Nombre total de binaires en conflit (`names` est plafonné).
    pub total: usize,
    pub names: Vec<ConflictName>,
}

const MAX_NAMES: usize = 30;

/// `entries` : (scope, index dans le scope, dossier développé) dans l'ordre de résolution.
/// `list_dir` renvoie les noms de fichiers (minuscules) d'un dossier.
pub fn find_conflicts(entries: &[(String, usize, String)], list_dir: &impl Fn(&str) -> Vec<String>) -> Vec<EntryConflicts> {
    let mut seen_dirs = HashSet::new();
    let mut by_name: HashMap<String, Vec<usize>> = HashMap::new(); // nom -> positions
    for (pos, (_, _, dir)) in entries.iter().enumerate() {
        if !seen_dirs.insert(dedup_key(dir)) {
            continue; // même dossier déjà vu : c'est un doublon, pas un conflit
        }
        let mut names: Vec<String> = list_dir(dir).into_iter().filter(|n| is_executable(n) && !is_noise(n)).collect();
        names.sort_unstable();
        names.dedup();
        for n in names {
            by_name.entry(n).or_default().push(pos);
        }
    }
    let mut per_entry: BTreeMap<usize, Vec<ConflictName>> = BTreeMap::new();
    for (name, positions) in by_name.into_iter().filter(|(_, p)| p.len() > 1) {
        let first = positions[0];
        for (k, &pos) in positions.iter().enumerate() {
            let other = if k == 0 { positions[1] } else { first };
            per_entry.entry(pos).or_default().push(ConflictName {
                name: name.clone(),
                wins: k == 0,
                other_scope: entries[other].0.clone(),
                other_index: entries[other].1,
            });
        }
    }
    per_entry
        .into_iter()
        .map(|(pos, mut names)| {
            names.sort_by(|a, b| a.name.cmp(&b.name));
            let total = names.len();
            names.truncate(MAX_NAMES);
            EntryConflicts { scope: entries[pos].0.clone(), index: entries[pos].1, total, names }
        })
        .collect()
}

fn list_dir_real(dir: &str) -> Vec<String> {
    let d = dir.trim().trim_matches('"');
    if d.starts_with(r"\\") {
        return vec![]; // chemin réseau : lent ou injoignable, jamais scanné
    }
    std::fs::read_dir(d)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_ok_and(|t| !t.is_dir()))
                .filter_map(|e| e.file_name().into_string().ok())
                .map(|n| n.to_lowercase())
                .collect()
        })
        .unwrap_or_default()
}

/// Repère les binaires de même nom présents dans plusieurs dossiers du PATH
/// (ex. plusieurs `python.exe`). Lecture seule.
#[tauri::command]
pub async fn analyze_conflicts() -> Vec<EntryConflicts> {
    let report = build_report();
    let entries: Vec<(String, usize, String)> = [("System", &report.system), ("User", &report.user)]
        .into_iter()
        .flat_map(|(s, r)| {
            r.entries.iter().enumerate().filter(|(_, e)| e.exists).map(move |(i, e)| (s.to_string(), i, e.expanded_path.clone()))
        })
        .collect();
    find_conflicts(&entries, &list_dir_real)
}

// ───────────────────────── Tests ─────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{analyze_registry_path, ScopeReport};

    fn lookup(n: &str) -> Option<String> {
        match n.to_lowercase().as_str() {
            "userprofile" => Some(r"C:\Users\Moi".into()),
            "localappdata" => Some(r"C:\Users\Moi\AppData\Local".into()),
            "appdata" => Some(r"C:\Users\Moi\AppData\Roaming".into()),
            "systemroot" => Some(r"C:\WINDOWS".into()),
            _ => None,
        }
    }
    /// Profil au nom long : `%USERPROFILE%` y raccourcit. (Avec « Moi » il rallongerait.)
    fn lookup_long(n: &str) -> Option<String> {
        match n.to_lowercase().as_str() {
            "userprofile" => Some(r"C:\Users\Utilisateur".into()),
            "localappdata" => Some(r"C:\Users\Utilisateur\AppData\Local".into()),
            "appdata" => Some(r"C:\Users\Utilisateur\AppData\Roaming".into()),
            other => lookup(other),
        }
    }
    fn report(user: &str, system: &str) -> RegistryPathReport {
        let mut seen = HashSet::new();
        let ex = |_: &str| true;
        let sys = analyze_registry_path("System", system, true, &lookup, &ex, &mut seen);
        let usr = analyze_registry_path("User", user, true, &lookup, &ex, &mut seen);
        let sr = |entries| ScopeReport { entries, value_type: Some("REG_EXPAND_SZ".into()), error: None };
        RegistryPathReport { user: sr(usr), system: sr(sys) }
    }

    #[test]
    fn seuils_de_risque() {
        let _l = crate::i18n::LANG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set_english(false);
        let lv = |n| assess(n).level;
        assert_eq!((lv(0), lv(500), lv(870)), ("ok", "ok", "ok")); // 85 % de 1024 = 870,4
        assert_eq!((lv(871), lv(1023)), ("near", "near"));
        assert_eq!((lv(1024), lv(2047)), ("medium", "medium"));
        assert_eq!((lv(2048), lv(8191)), ("high", "high"));
        assert_eq!(lv(8192), "critical");
        assert!(assess(1000).message.contains("24 caractères de marge"));
    }

    #[test]
    fn risque_en_anglais() {
        let _l = crate::i18n::LANG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set_english(true);
        let (near, high) = (assess(1000).message, assess(3000).message);
        crate::i18n::set_english(false);
        assert!(near.contains("24 characters of margin") && high.starts_with("High:"));
    }

    #[test]
    fn longueur_en_unites_utf16_et_pas_en_octets() {
        assert_eq!(utf16_len("C:\\é"), 4); // 5 octets UTF-8, 4 unités UTF-16
        assert_eq!(utf16_len("😀"), 2);
    }

    #[test]
    fn rapport_de_longueur_brut_et_effectif() {
        let r = build_length_report(Some((r"%USERPROFILE%\bin", true)), Some((r"%SystemRoot%\system32", true)), &lookup);
        assert_eq!(r.user.length, 17); // texte brut
        assert_eq!(r.system.length, 21);
        // effectif : développé, jointure par « ; »
        assert_eq!(r.combined.length, r"C:\WINDOWS\system32".len() + 1 + r"C:\Users\Moi\bin".len());
        let s = build_length_report(Some((r"%USERPROFILE%\bin", false)), None, &lookup); // REG_SZ : pas développé
        assert_eq!(s.combined.length, 17);
        assert_eq!(build_length_report(None, None, &lookup).combined.length, 0);
    }

    #[test]
    fn compression_respecte_le_scope_les_frontieres_et_ne_rallonge_jamais() {
        let subs = substitutions(&lookup_long);
        let c = |raw: &str, scope: &str| compress_entry(raw, scope, &subs);
        // Préfixe le plus long d'abord : LOCALAPPDATA avant USERPROFILE.
        assert_eq!(c(r"C:\Users\Utilisateur\AppData\Local\bin", "User"), r"%LOCALAPPDATA%\bin");
        assert_eq!(c(r"C:\Users\Utilisateur\AppData\Roaming\npm", "User"), r"%APPDATA%\npm");
        assert_eq!(c(r"C:\Users\Utilisateur\.cargo\bin", "User"), r"%USERPROFILE%\.cargo\bin");
        assert_eq!(c(r"c:\users\UTILISATEUR", "User"), "%USERPROFILE%"); // casse ignorée
        assert_eq!(c(r"C:\Users\UtilisateurEtAutre\x", "User"), r"C:\Users\UtilisateurEtAutre\x"); // pas une frontière
        // Jamais de variable propre à l'utilisateur dans le PATH Système.
        assert_eq!(c(r"C:\Users\Utilisateur\bin", "System"), r"C:\Users\Utilisateur\bin");
        assert_eq!(c(r"C:\Users\Utilisateur\AppData\Local\x", "System"), r"C:\Users\Utilisateur\AppData\Local\x");
        // Windows : %SystemRoot% rallonge (10 → 12), %windir% raccourcit (10 → 8).
        assert_eq!(c(r"C:\Windows\System32", "System"), r"%windir%\System32");
        assert_eq!(c(r"C:\WINDOWS", "User"), "%windir%");
        // Rien à gagner : un profil court (« Moi ») ne rallonge pas.
        let short = substitutions(&lookup);
        assert_eq!(compress_entry(r"C:\Users\Moi\bin", "User", &short), r"C:\Users\Moi\bin");
        // Déjà variabilisé, entre guillemets, ou hors des dossiers connus : intact.
        assert_eq!(c(r"%SystemRoot%\system32", "System"), r"%SystemRoot%\system32");
        assert_eq!(c(r#""C:\Users\Utilisateur\a b""#, "User"), r#""C:\Users\Utilisateur\a b""#);
        assert_eq!(c(r"D:\Tools", "User"), r"D:\Tools");
        // Dossier trop court (« C:\ ») : ignoré.
        let tiny = vec![Substitution { token: "%X%", dir: "C:\\".into(), user_only: false }];
        assert_eq!(compress_entry(r"C:\x", "User", &tiny), r"C:\x");
    }

    #[test]
    fn plan_de_compression_et_gain() {
        let subs = substitutions(&lookup_long);
        let r = report(r"C:\Users\Utilisateur\bin;D:\z;%USERPROFILE%\ok", r"C:\Windows\System32;C:\Users\Utilisateur\x");
        let ch = plan_compress(&r, ScopeSel::All, &subs);
        let got: Vec<(&str, usize, &str)> = ch.iter().map(|c| (c.scope.as_str(), c.index, c.new.as_str())).collect();
        assert_eq!(got, vec![("System", 0, r"%windir%\System32"), ("User", 0, r"%USERPROFILE%\bin")]);
        assert_eq!(saved(&ch), 2 + (r"C:\Users\Utilisateur".len() - "%USERPROFILE%".len()));
        assert_eq!(plan_compress(&r, ScopeSel::System, &subs).len(), 1);
        assert!(plan_compress(&r, ScopeSel::All, &[]).is_empty());
    }

    #[test]
    fn reordonnancement() {
        let v: Vec<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        assert_eq!(swap_entries(&v, 1, "up").unwrap(), vec!["b", "a", "c"]);
        assert_eq!(swap_entries(&v, 1, "down").unwrap(), vec!["a", "c", "b"]);
        assert!(swap_entries(&v, 0, "up").is_err() && swap_entries(&v, 2, "down").is_err());
        assert!(swap_entries(&v, 3, "up").is_err() && swap_entries(&v, 1, "left").is_err());
        assert_eq!(join_raw(swap_entries(&v, 0, "down").unwrap()), "b;a;c");
    }

    #[test]
    fn reutilisation_de_la_sauvegarde_recente() {
        assert!(can_reuse_backup("avant-reordonnancement", Some(10), "avant-reordonnancement", 300));
        assert!(!can_reuse_backup("avant-reordonnancement", Some(300), "avant-reordonnancement", 300)); // trop vieille
        assert!(!can_reuse_backup("manuelle", Some(10), "avant-reordonnancement", 300)); // autre raison
        assert!(!can_reuse_backup("avant-reordonnancement", None, "avant-reordonnancement", 300)); // date illisible
        assert!(!can_reuse_backup("avant-reordonnancement", Some(-5), "avant-reordonnancement", 300)); // date future
    }

    #[test]
    fn conflits_de_binaires() {
        let entries: Vec<(String, usize, String)> = [
            ("System", 0, r"C:\A"),
            ("System", 1, r"C:\B"),
            ("User", 0, r"C:\a\"), // même dossier que C:\A : doublon, jamais un conflit
            ("User", 1, r"C:\C"),
            ("User", 2, r"C:\Vide"),
        ]
        .iter()
        .map(|(s, i, d)| (s.to_string(), *i, d.to_string()))
        .collect();
        let list = |d: &str| -> Vec<String> {
            match d.to_lowercase().trim_end_matches('\\') {
                r"c:\a" => vec!["python.exe", "node.exe", "readme.txt", "setup.exe", "A.cmd"],
                r"c:\b" => vec!["python.exe", "tool.exe"],
                r"c:\c" => vec!["python.exe", "node.exe", "setup.exe"],
                _ => vec![],
            }
            .into_iter()
            .map(|s| s.to_lowercase())
            .collect()
        };
        let r = find_conflicts(&entries, &list);
        let got: Vec<(&str, usize, usize)> = r.iter().map(|e| (e.scope.as_str(), e.index, e.total)).collect();
        // A: python (gagne) + node (gagne) ; B: python (masqué) ; C: python + node (masqués). setup.exe = bruit.
        assert_eq!(got, vec![("System", 0, 2), ("System", 1, 1), ("User", 1, 2)]);
        let a = &r[0].names;
        // node.exe : A gagne, l'autre dossier qui le contient est C = (User, 1).
        assert_eq!((a[0].name.as_str(), a[0].wins, a[0].other_scope.as_str(), a[0].other_index), ("node.exe", true, "User", 1));
        // python.exe : A gagne, le premier masqué est B = (System, 1).
        assert_eq!((a[1].name.as_str(), a[1].wins, a[1].other_scope.as_str(), a[1].other_index), ("python.exe", true, "System", 1));
    }

    #[test]
    fn conflit_autre_entree_pointe_le_gagnant() {
        let entries: Vec<(String, usize, String)> =
            vec![("System".into(), 0, r"C:\A".into()), ("User".into(), 3, r"C:\B".into())];
        let list = |d: &str| if d == r"C:\A" || d == r"C:\B" { vec!["node.exe".to_string()] } else { vec![] };
        let r = find_conflicts(&entries, &list);
        assert_eq!(r.len(), 2);
        assert!(r[0].names[0].wins && r[0].names[0].other_scope == "User" && r[0].names[0].other_index == 3);
        assert!(!r[1].names[0].wins && r[1].names[0].other_scope == "System" && r[1].names[0].other_index == 0);
    }

    #[test]
    fn detection_des_executables_et_du_bruit() {
        assert!(is_executable("python.exe") && is_executable("run.cmd") && is_executable("a.bat") && is_executable("x.com"));
        assert!(!is_executable("readme.txt") && !is_executable("python") && !is_executable("lib.dll"));
        assert!(is_noise("unins000.exe") && is_noise("setup.exe") && is_noise("update.exe") && !is_noise("node.exe"));
    }

    #[test]
    fn lecture_reelle_des_conflits_et_longueurs_ne_plante_pas() {
        let c = tauri::async_runtime::block_on(analyze_conflicts());
        assert!(c.iter().all(|e| e.total >= 1 && e.names.len() <= MAX_NAMES));
        let l = tauri::async_runtime::block_on(get_path_lengths()).unwrap();
        assert!(l.system.length > 0 && l.combined.length >= l.system.length);
    }
}
