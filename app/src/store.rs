//! Per-monitor preferences saved by unique key (nicknames, sync group, profiles).

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// A named set of DDC/CI values (VCP code → value) the person can apply in one click.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub values: BTreeMap<u8, u32>,
}

#[derive(Default, Serialize, Deserialize)]
pub struct Store {
    /// unique key → nickname the person gave the monitor
    #[serde(default)]
    pub nicknames: BTreeMap<String, String>,
    /// unique keys that follow changes made to the selected monitor
    #[serde(default)]
    pub sync: BTreeSet<String>,
    /// last selected monitor
    #[serde(default)]
    pub last_selected: Option<String>,
    /// saved profiles, shared by every monitor
    #[serde(default)]
    pub profiles: Vec<Profile>,
    #[serde(skip)]
    path: Option<PathBuf>,
}

const DIR: &str = "MonitorControlCenter";
/// Folder used by versions up to 0.1.2; read once if the new one has no settings yet.
const OLD_DIR: &str = "LegionMonitorControl";

impl Store {
    pub fn load() -> Self {
        let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let path = base.join(DIR).join("settings.json");
        let text = std::fs::read_to_string(&path)
            .or_else(|_| std::fs::read_to_string(base.join(OLD_DIR).join("settings.json")))
            .ok();
        let mut s: Store = text.and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
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
