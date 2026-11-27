//! Settings that must survive restarts (theme, indent, font size, last folder, recent files).
//! One small JSON file in the user's config directory: no database, nothing else is stored.

use serde_json::{Map, Value};
use std::path::PathBuf;
use std::sync::Mutex;

/// Only these keys are ever stored.
const KEYS: &[&str] = &["theme", "indent", "fs", "dir", "recent", "tdelim", "theader", "astext", "expand", "readonly", "zones", "clock12", "hols", "holc", "sound", "appwin", "notify", "pinned", "recentAt", "session", "notifyAsked", "season", "browser", "fold", "dstx", "dsta", "meet", "ws", "dfmt", "hdf", "keys", "updchk", "updlast", "updseen", "updrepo", "ncat", "nfilt", "nsort", "ndone", "wrap", "accent", "mono", "showhid"];
const MAX_BYTES: usize = 64 * 1024;

/// Checklist items and the scratchpad live in their own file so they never compete with the settings for space.
const NOTE_KEYS: &[&str] = &["items", "pad", "docs"];
const NOTE_MAX_BYTES: usize = 512 * 1024;

/// The calendar lives in its own file too: events (with sync-ready ids, change times and deletion markers) and view settings.
const CAL_KEYS: &[&str] = &["events", "cfg"];
const CAL_MAX_BYTES: usize = 4 * 1024 * 1024;

pub struct Store {
    path: PathBuf,
    encrypt: bool,
    lock: Mutex<()>,
    keys: &'static [&'static str],
    max: usize,
}

impl Store {
    pub fn new() -> Store {
        Store { path: crate::plat::config_dir().join("state.json"), lock: Mutex::new(()), encrypt: false, keys: KEYS, max: MAX_BYTES }
    }

    /// the notes file: checklist, reminders and the scratchpad
    pub fn notes() -> Store {
        Store { path: crate::plat::config_dir().join("notes.json"), lock: Mutex::new(()), encrypt: true, keys: NOTE_KEYS, max: NOTE_MAX_BYTES }
    }

    /// the calendar file
    pub fn calendar() -> Store {
        Store { path: crate::plat::config_dir().join("calendar.json"), lock: Mutex::new(()), encrypt: false, keys: CAL_KEYS, max: CAL_MAX_BYTES }
    }

    fn read(&self) -> Map<String, Value> {
        std::fs::read(&self.path)
            .ok()
            .and_then(|b| if b.starts_with(MAGIC) { open_sealed(&b) } else { String::from_utf8(b).ok() })
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
        let bytes = if self.encrypt { seal(text.as_bytes())? } else { text.into_bytes() };
        std::fs::write(&tmp, bytes).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &self.path).map_err(|e| format!("cannot write {}: {e}", self.path.display()))?;
        Ok(Value::Object(m))
    }
}

/// Notes are stored encrypted (AES-256-GCM). The key is made on first use and kept in its own owner-only file,
/// so copying `notes.json` alone (a backup, a synced folder, a stray upload) reveals nothing.
const MAGIC: &[u8] = b"KEMPTENC1\n";

fn key_path() -> PathBuf {
    crate::plat::config_dir().join("keys").join("notes.key")
}

fn load_key() -> Result<[u8; 32], String> {
    let p = key_path();
    if let Ok(b) = std::fs::read(&p) {
        if b.len() == 32 {
            let mut k = [0u8; 32];
            k.copy_from_slice(&b);
            return Ok(k);
        }
    }
    let mut k = [0u8; 32];
    getrandom::fill(&mut k).map_err(|e| format!("no random source: {e}"))?;
    std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
    let tmp = p.with_extension("tmp");
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp).map_err(|e| e.to_string())?;
        f.write_all(&k).map_err(|e| e.to_string())?;
        let _ = std::fs::set_permissions(p.parent().unwrap(), std::fs::Permissions::from_mode(0o700));
    }
    #[cfg(not(unix))]
    std::fs::write(&tmp, k).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &p).map_err(|e| e.to_string())?;
    Ok(k)
}

fn seal(plain: &[u8]) -> Result<Vec<u8>, String> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};
    let key = load_key()?;
    let mut nonce = [0u8; 12];
    getrandom::fill(&mut nonce).map_err(|e| format!("no random source: {e}"))?;
    let c = Aes256Gcm::new_from_slice(&key).map_err(|e| e.to_string())?;
    let ct = c.encrypt(Nonce::from_slice(&nonce), plain).map_err(|_| "encryption failed".to_string())?;
    let mut out = MAGIC.to_vec();
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

fn open_sealed(b: &[u8]) -> Option<String> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};
    let body = b.strip_prefix(MAGIC)?;
    if body.len() < 12 + 16 {
        return None;
    }
    let key = std::fs::read(key_path()).ok().filter(|k| k.len() == 32)?;
    let c = Aes256Gcm::new_from_slice(&key).ok()?;
    let pt = c.decrypt(Nonce::from_slice(&body[..12]), &body[12..]).ok()?;
    String::from_utf8(pt).ok()
}

/// `kempt notes-export`: the decrypted notes file as JSON (for backups and scripts).
pub fn export_notes() -> String {
    serde_json::to_string_pretty(&Store::notes().get()).unwrap_or_default()
}
