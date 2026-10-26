//! An instance: one process of the application, from start to exit. A ledger
//! keeps each instance in its own directory, so a restart, or a second replica
//! running at the same time, never writes into another process's file.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::die;
use crate::otlp::{iso_utc, now_nanos, random_hex};

pub const SPANS: &str = "spans.jsonl";
pub const PINNED: &str = "exe";
const META: &str = "instance.json";

pub struct Instance {
    pub dir: PathBuf,
    /// `<UTC start, to the second>Z-<4 hex>`: sorts by start, unique within its ledger.
    pub id: String,
}

impl Instance {
    fn at(dir: PathBuf) -> Instance {
        let id = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Instance { dir, id }
    }

    /// A new instance directory in `ledger`, which is created if missing.
    pub fn create(ledger: &Path) -> Instance {
        if ledger.join(SPANS).exists() {
            die(73, &format!("{} is a one-process ledger from before instances; record into a new directory", ledger.display()));
        }
        fs::create_dir_all(ledger).unwrap_or_else(|e| die(73, &format!("cannot create {}: {e}", ledger.display())));
        let started: String = iso_utc(now_nanos())[..19].chars().filter(char::is_ascii_alphanumeric).collect();
        for _ in 0..16 {
            let dir = ledger.join(format!("{started}Z-{}", random_hex(2)));
            match fs::create_dir(&dir) {
                Ok(()) => return Instance::at(dir),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => die(73, &format!("cannot create {}: {e}", dir.display())),
            }
        }
        die(73, &format!("cannot name a new instance in {}", ledger.display()))
    }

    /// The instances under `path`, oldest first. `path` may be one instance itself.
    pub fn list(path: &Path) -> Vec<Instance> {
        if path.join(SPANS).exists() {
            return vec![Instance::at(path.to_path_buf())];
        }
        let mut found: Vec<Instance> = fs::read_dir(path)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.join(SPANS).exists())
            .map(Instance::at)
            .collect();
        found.sort_by(|a, b| a.id.cmp(&b.id));
        found
    }

    /// The part of the id a dump shows.
    pub fn short(&self) -> &str {
        self.id.rsplit('-').next().unwrap_or(&self.id)
    }

    pub fn meta(&self) -> Value {
        fs::read_to_string(self.dir.join(META)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null)
    }

    pub fn write_meta(&self, meta: &Value) {
        let _ = fs::write(self.dir.join(META), format!("{meta}\n"));
    }
}
