use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;

/// Une entrée du %PATH% avec son état.
#[derive(Serialize, Debug, PartialEq)]
pub struct PathEntry {
    pub path: String,
    pub exists: bool,
    pub is_duplicate: bool,
}

/// Découpe une chaîne PATH (séparateur `;`) et marque existence et doublons.
/// Les doublons sont détectés sans tenir compte de la casse, des `/` vs `\`
/// ni du `\` final (Windows). La première occurrence n'est pas un doublon.
/// `exists` reçoit le test d'existence, injecté pour pouvoir tester sans disque.
pub fn analyze_path(raw: &str, exists: impl Fn(&str) -> bool) -> Vec<PathEntry> {
    let mut seen: HashSet<String> = HashSet::new();
    raw.split(';')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| {
            let key = p.replace('/', "\\").trim_end_matches('\\').to_lowercase();
            let is_duplicate = !seen.insert(key);
            PathEntry {
                path: p.to_string(),
                exists: exists(p),
                is_duplicate,
            }
        })
        .collect()
}

/// Lecture seule : lit le %PATH% du processus et le analyse.
#[tauri::command]
fn get_path_entries() -> Vec<PathEntry> {
    let raw = std::env::var("PATH").unwrap_or_default();
    analyze_path(&raw, |p| Path::new(p).exists())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_path_entries])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marque_les_doublons_sans_tenir_compte_de_la_casse() {
        let r = analyze_path(r"C:\Bin;c:\bin\;D:\Tools;C:\BIN", |_| true);
        let dup: Vec<bool> = r.iter().map(|e| e.is_duplicate).collect();
        assert_eq!(dup, vec![false, true, false, true]);
    }

    #[test]
    fn ignore_les_segments_vides_et_marque_les_orphelins() {
        let r = analyze_path(r"C:\Ok;;  ;C:\Absent", |p| p == r"C:\Ok");
        assert_eq!(r.len(), 2);
        assert!(r[0].exists);
        assert!(!r[1].exists);
    }

    #[test]
    fn path_vide() {
        assert!(analyze_path("", |_| true).is_empty());
    }
}
