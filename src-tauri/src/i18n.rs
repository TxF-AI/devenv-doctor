//! Langue des messages renvoyés par le backend (erreurs, libellés du tray).
//! Le français reste la langue par défaut ; l'interface appelle `set_language` au démarrage.

use std::sync::atomic::{AtomicBool, Ordering};

static ENGLISH: AtomicBool = AtomicBool::new(false);

pub fn set_english(on: bool) {
    ENGLISH.store(on, Ordering::Relaxed);
}

pub fn is_english() -> bool {
    ENGLISH.load(Ordering::Relaxed)
}

/// `tr!(français, english)` : une seule des deux branches est évaluée.
macro_rules! tr {
    ($fr:expr, $en:expr) => {
        if $crate::i18n::is_english() { $en } else { $fr }
    };
}

/// La langue est globale : les tests qui lisent un message la figent le temps du test.
#[cfg(test)]
pub static LANG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
