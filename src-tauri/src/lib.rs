mod actions;
mod profiles;

use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, REG_EXPAND_SZ};
use winreg::{RegKey, HKEY};

const USER_KEY: &str = "Environment";
const SYSTEM_KEY: &str = r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment";

/// Une entrée du PATH lue dans le Registre, avec son état.
#[derive(Serialize, Debug, PartialEq)]
pub struct PathEntry {
    /// "User" (HKCU) ou "System" (HKLM).
    pub scope: String,
    /// Texte exact du Registre (ex. `%USERPROFILE%\bin`).
    pub raw_path: String,
    /// Chemin après expansion des `%VAR%` (identique à `raw_path` pour un REG_SZ).
    pub expanded_path: String,
    pub exists: bool,
    pub is_duplicate: bool,
}

/// Contenu d'une des deux valeurs `Path`.
#[derive(Serialize, Debug, PartialEq)]
pub struct ScopeReport {
    pub entries: Vec<PathEntry>,
    /// "REG_SZ", "REG_EXPAND_SZ", "Autre" ; None si la valeur est absente.
    pub value_type: Option<String>,
    /// Message d'erreur (accès refusé, etc.) ; None si la lecture a réussi.
    pub error: Option<String>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct RegistryPathReport {
    pub user: ScopeReport,
    pub system: ScopeReport,
}

/// Développe les `%VAR%` de `s`. Une variable inconnue reste telle quelle
/// (comme `ExpandEnvironmentStrings`) ; un `%` isolé est conservé.
pub fn expand_env(s: &str, lookup: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) if end > 0 => match lookup(&after[..end]) {
                Some(v) => {
                    out.push_str(&v);
                    rest = &after[end + 1..];
                }
                None => {
                    // Variable inconnue : garder `%NOM%` et reprendre après le 1er `%`.
                    out.push('%');
                    rest = after;
                }
            },
            _ => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Décode les octets d'une valeur de Registre texte (UTF-16 LE, NUL final retiré).
pub fn decode_reg_string(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    String::from_utf16_lossy(&units)
        .trim_end_matches('\0')
        .to_string()
}

fn dedup_key(expanded: &str) -> String {
    expanded
        .trim()
        .trim_matches('"')
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

/// Analyse la valeur `Path` d'UN scope. `expandable` = la valeur est un
/// REG_EXPAND_SZ ; sinon Windows ne développe pas les `%VAR%` et nous non plus.
/// `seen` est partagé entre scopes pour repérer aussi un doublon Utilisateur/Système.
pub fn analyze_registry_path(
    scope: &str,
    raw: &str,
    expandable: bool,
    lookup: &impl Fn(&str) -> Option<String>,
    exists: &impl Fn(&str) -> bool,
    seen: &mut HashSet<String>,
) -> Vec<PathEntry> {
    raw.split(';')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| {
            let expanded = if expandable {
                expand_env(p, lookup)
            } else {
                p.to_string()
            };
            let is_duplicate = !seen.insert(dedup_key(&expanded));
            PathEntry {
                scope: scope.to_string(),
                raw_path: p.to_string(),
                exists: exists(expanded.trim().trim_matches('"')),
                expanded_path: expanded,
                is_duplicate,
            }
        })
        .collect()
}

/// Lecture seule d'une valeur `Path` : (texte, est_expandable, type). `None` si absente.
fn read_path_value(root: HKEY, subkey: &str) -> Result<Option<(String, bool, String)>, String> {
    let hive = RegKey::predef(root);
    let key = match hive.open_subkey_with_flags(subkey, KEY_READ) {
        Ok(k) => k,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    match key.get_raw_value("Path") {
        Ok(v) => {
            let expandable = v.vtype == REG_EXPAND_SZ;
            let type_name = match v.vtype {
                REG_EXPAND_SZ => "REG_EXPAND_SZ",
                winreg::enums::REG_SZ => "REG_SZ",
                _ => "Autre",
            };
            Ok(Some((
                decode_reg_string(&v.bytes[..]),
                expandable,
                type_name.to_string(),
            )))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

fn build_report() -> RegistryPathReport {
    let lookup = |name: &str| std::env::var(name).ok();
    let exists = |p: &str| Path::new(p).exists();
    let mut seen = HashSet::new();

    // Ordre de résolution Windows : Système puis Utilisateur. Le premier gagne,
    // donc c'est l'entrée Utilisateur répétée qui est marquée « doublon ».
    let mut scope_report = |root: HKEY, key: &str, scope: &str| match read_path_value(root, key) {
        Ok(Some((raw, expandable, ty))) => ScopeReport {
            entries: analyze_registry_path(scope, &raw, expandable, &lookup, &exists, &mut seen),
            value_type: Some(ty),
            error: None,
        },
        Ok(None) => ScopeReport { entries: vec![], value_type: None, error: None },
        Err(e) => ScopeReport { entries: vec![], value_type: None, error: Some(e) },
    };
    let system = scope_report(HKEY_LOCAL_MACHINE, SYSTEM_KEY, "System");
    let user = scope_report(HKEY_CURRENT_USER, USER_KEY, "User");
    RegistryPathReport { user, system }
}

/// Lecture seule : PATH Utilisateur (HKCU) et Système (HKLM) depuis le Registre.
#[tauri::command]
async fn get_registry_paths() -> RegistryPathReport {
    build_report()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_registry_paths,
            actions::is_admin,
            actions::create_registry_backup,
            actions::restore_registry_backup,
            actions::list_backups,
            actions::preview_restore,
            actions::preview_cleanup,
            actions::remove_orphans,
            actions::deduplicate_paths,
            profiles::list_presets,
            profiles::export_profile,
            profiles::import_profile,
            profiles::preview_profile,
            profiles::apply_profile
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(name: &str) -> Option<String> {
        match name.to_lowercase().as_str() {
            "userprofile" => Some(r"C:\Users\Moi".to_string()),
            "systemroot" => Some(r"C:\Windows".to_string()),
            _ => None,
        }
    }

    #[test]
    fn expand_remplace_les_variables_connues() {
        assert_eq!(
            expand_env(r"%USERPROFILE%\AppData\%SystemRoot%", env),
            r"C:\Users\Moi\AppData\C:\Windows"
        );
    }

    #[test]
    fn expand_garde_variable_inconnue_et_pourcent_isole() {
        assert_eq!(expand_env(r"%INCONNUE%\bin", env), r"%INCONNUE%\bin");
        assert_eq!(expand_env("100%", env), "100%");
        assert_eq!(expand_env("%%x", env), "%%x");
        assert_eq!(expand_env(r"%INCONNUE%%USERPROFILE%", env), r"%INCONNUE%C:\Users\Moi");
    }

    #[test]
    fn decode_utf16_retire_le_nul_final() {
        let mut b: Vec<u8> = r"C:\a;%X%".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        b.extend_from_slice(&[0, 0]);
        assert_eq!(decode_reg_string(&b), r"C:\a;%X%");
    }

    #[test]
    fn reg_expand_sz_developpe_et_reg_sz_ne_developpe_pas() {
        let ex = |_: &str| true;
        let mut s = HashSet::new();
        let a = analyze_registry_path("User", r"%USERPROFILE%\bin", true, &env, &ex, &mut s);
        assert_eq!(a[0].expanded_path, r"C:\Users\Moi\bin");
        assert_eq!(a[0].raw_path, r"%USERPROFILE%\bin");

        let mut s = HashSet::new();
        let b = analyze_registry_path("User", r"%USERPROFILE%\bin", false, &env, &ex, &mut s);
        assert_eq!(b[0].expanded_path, r"%USERPROFILE%\bin");
    }

    #[test]
    fn doublon_dans_un_scope_et_entre_scopes() {
        let ex = |_: &str| true;
        let mut s = HashSet::new();
        let sys = analyze_registry_path("System", r"C:\Bin;c:\bin\", true, &env, &ex, &mut s);
        let usr = analyze_registry_path("User", r"%SystemRoot%;C:\WINDOWS\", true, &env, &ex, &mut s);
        assert_eq!(sys.iter().map(|e| e.is_duplicate).collect::<Vec<_>>(), vec![false, true]);
        // %SystemRoot% (= C:\Windows) est nouveau ; sa variante avec « \ » final est un doublon.
        assert_eq!(usr.iter().map(|e| e.is_duplicate).collect::<Vec<_>>(), vec![false, true]);
        assert!(usr.iter().all(|e| e.scope == "User"));
    }

    #[test]
    fn existence_testee_sur_le_chemin_developpe_sans_guillemets() {
        let ex = |p: &str| p == r"C:\Users\Moi\bin";
        let mut s = HashSet::new();
        let r = analyze_registry_path(
            "User",
            r#""%USERPROFILE%\bin";C:\Absent;;"#,
            true,
            &env,
            &ex,
            &mut s,
        );
        assert_eq!(r.len(), 2);
        assert!(r[0].exists);
        assert!(!r[1].exists);
    }

    #[test]
    fn lecture_reelle_du_registre_ne_plante_pas() {
        // Lecture seule ; le HKLM Path existe sur toute machine Windows.
        let r = build_report();
        assert!(r.system.error.is_none());
        assert!(r.system.value_type.is_some());
        assert!(!r.system.entries.is_empty());
    }
}
