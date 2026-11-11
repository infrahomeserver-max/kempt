//! Settings that must survive restarts (theme, indent, font size, last folder, recent files).
//! One small JSON file in the user's config directory: no database, nothing else is stored.

use serde_json::{Map, Value};
use std::path::PathBuf;
use std::sync::Mutex;

/// Only these keys are ever stored.
const KEYS: &[&str] = &["theme", "indent", "fs", "dir", "recent", "tdelim", "theader", "astext", "expand", "readonly", "zones", "clock12", "hols", "holc", "sound", "appwin", "season", "browser", "fold", "dstx", "dsta", "meet", "updchk", "updlast", "updseen", "updrepo", "ncat", "nfilt"];
const MAX_BYTES: usize = 64 * 1024;

/// Checklist items and the scratchpad live in their own file so they never compete with the settings for space.
const NOTE_KEYS: &[&str] = &["items", "pad"];
const NOTE_MAX_BYTES: usize = 512 * 1024;

pub struct Store {
    path: PathBuf,
    lock: Mutex<()>,
    keys: &'static [&'static str],
    max: usize,
}

impl Store {
    pub fn new() -> Store {
        Store { path: crate::plat::config_dir().join("state.json"), lock: Mutex::new(()), keys: KEYS, max: MAX_BYTES }
    }

    /// the notes file: checklist, reminders and the scratchpad
    pub fn notes() -> Store {
        Store { path: crate::plat::config_dir().join("notes.json"), lock: Mutex::new(()), keys: NOTE_KEYS, max: NOTE_MAX_BYTES }
    }

    fn read(&self) -> Map<String, Value> {
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| if let Value::Object(m) = v { Some(m) } else { None })
            .unwrap_or_default()
    }

    pub fn get(&self) -> Value {
        let _g = self.lock.lock().unwrap();
        Value::Object(self.read())
    }

    /// Merge `patch` (unknown keys are ignored; `null` removes a key) and write the file atomically.
    pub fn set(&self, patch: &Value) -> Result<Value, String> {
        self.modify(|m| {
            if let Value::Object(p) = patch {
                for (k, v) in p {
                    if !self.keys.contains(&k.as_str()) {
                        continue;
                    }
                    if v.is_null() {
                        m.remove(k);
                    } else {
                        m.insert(k.clone(), v.clone());
                    }
                }
            }
        })
    }

    /// Read, change and write back under one lock (used by the reminder thread to flag items it has announced).
    pub fn modify(&self, f: impl FnOnce(&mut Map<String, Value>)) -> Result<Value, String> {
        let _g = self.lock.lock().unwrap();
        let mut m = self.read();
        f(&mut m);
        let text = serde_json::to_string_pretty(&Value::Object(m.clone())).map_err(|e| e.to_string())?;
        if text.len() > self.max {
            return Err("settings too large".into());
        }
        let dir = self.path.parent().unwrap();
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, text).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &self.path).map_err(|e| format!("cannot write {}: {e}", self.path.display()))?;
        Ok(Value::Object(m))
    }
}
