//! Per-monitor preferences saved by unique key (nicknames and sync group).

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

#[derive(Default, Serialize, Deserialize)]
pub struct Store {
    /// unique key → nickname the person gave the monitor
    pub nicknames: BTreeMap<String, String>,
    /// unique keys that follow changes made to the selected monitor
    pub sync: BTreeSet<String>,
    /// last selected monitor
    pub last_selected: Option<String>,
    #[serde(skip)]
    path: Option<PathBuf>,
}

impl Store {
    pub fn load() -> Self {
        let path = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("LegionMonitorControl")
            .join("settings.json");
        let mut s: Store = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        s.path = Some(path);
        s
    }

    pub fn save(&self) {
        if let Some(p) = &self.path {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(t) = serde_json::to_string_pretty(self) {
                let _ = std::fs::write(p, t);
            }
        }
    }
}
