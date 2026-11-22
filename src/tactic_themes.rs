//! The shared vocabulary for tactic import and theme settings.

use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, sync::LazyLock};

#[derive(Deserialize, Serialize)]
pub struct TacticTheme {
    pub id: String,
    pub name: String,
    group: String,
}

pub fn tactic_themes() -> &'static [TacticTheme] {
    static THEMES: LazyLock<Vec<TacticTheme>> = LazyLock::new(|| {
        serde_json::from_str(include_str!("tactic_themes.json")).expect("bundled Tempo theme list")
    });
    &THEMES
}

pub fn known_tactic_theme(id: &str) -> bool {
    static IDS: LazyLock<BTreeSet<&'static str>> = LazyLock::new(|| {
        tactic_themes()
            .iter()
            .map(|theme| theme.id.as_str())
            .collect()
    });
    IDS.contains(id)
}
