use chrono::Local;
use regex::Regex;
use serde_json::{json, Value};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};

pub struct Journal {
    dir: PathBuf,
    secrets: Mutex<Vec<String>>,
    write_lock: Mutex<()>,
}
impl Journal {
    pub fn new(dir: &Path) -> Self {
        let _ = fs::create_dir_all(dir);
        Self {
            dir: dir.into(),
            secrets: Mutex::new(vec![]),
            write_lock: Mutex::new(()),
        }
    }
    pub fn register(&self, values: impl IntoIterator<Item = String>) {
        self.secrets
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .extend(values.into_iter().filter(|s| !s.is_empty()));
    }
    pub fn redact(&self, text: &str) -> String {
        let mut safe = text.to_string();
        for s in self
            .secrets
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
        {
            safe = safe.replace(s, "[скрыто]");
        }
        for pattern in [
            r#"(?i)(?:https?|naive\+https|sing-box)://[^\s"'<>]+"#,
            r"(?i)Bearer\s+[A-Za-z0-9._~+/=-]+",
            r"(?i)Basic\s+[A-Za-z0-9+/=]+",
        ] {
            safe = Regex::new(pattern)
                .unwrap()
                .replace_all(&safe, "[скрыто]")
                .into_owned();
        }
        safe
    }
    pub fn event(&self, level: &str, code: &str, message: &str) {
        let _lock = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());
        let path = self.dir.join("app.jsonl");
        if fs::metadata(&path)
            .map(|m| m.len() > 1_048_576)
            .unwrap_or(false)
        {
            let _ = fs::remove_file(self.dir.join("app.3.jsonl"));
            for n in (1..3).rev() {
                let _ = fs::rename(
                    self.dir.join(format!("app.{n}.jsonl")),
                    self.dir.join(format!("app.{}.jsonl", n + 1)),
                );
            }
            let _ = fs::rename(&path, self.dir.join("app.1.jsonl"));
        }
        let message: String = self.redact(message).chars().take(16_384).collect();
        let v =
            json!({"time":Local::now().to_rfc3339(),"level":level,"code":code,"message":message});
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{v}");
        }
    }
    pub fn read(&self) -> Vec<Value> {
        let _lock = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());
        fs::read_to_string(self.dir.join("app.jsonl"))
            .unwrap_or_default()
            .lines()
            .rev()
            .take(200)
            .filter_map(|s| serde_json::from_str::<Value>(&self.redact(s)).ok())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persist_and_redact_credentials() {
        let d = tempfile::tempdir().unwrap();
        let log = Journal::new(d.path());
        log.register(vec!["private-password".into()]);
        log.event(
            "error",
            "CP-TEST",
            "Failed private-password https://user:pass@private.example/subsecret Bearer abc123",
        );
        let saved = fs::read_to_string(d.path().join("app.jsonl")).unwrap();
        for secret in ["private-password", "private.example", "abc123"] {
            assert!(!saved.contains(secret));
        }
        assert_eq!(log.read().len(), 1);
    }
    #[test]
    fn long_tracebacks_and_large_reports_stay_valid_json() {
        let d = tempfile::tempdir().unwrap();
        let log = Journal::new(d.path());
        log.event("error", "CP-LONG", &"stack-frame ".repeat(4000));
        assert_eq!(log.read().len(), 1);
        let report = json!({"logs":log.read(),"padding":"diagnostic-data".repeat(3000)});
        let redacted = log.redact(&report.to_string());
        assert!(redacted.len() > 16384);
        assert!(serde_json::from_str::<Value>(&redacted).is_ok());
    }
    #[test]
    fn rotates_without_losing_new_error() {
        let d = tempfile::tempdir().unwrap();
        let log = Journal::new(d.path());
        fs::write(d.path().join("app.jsonl"), vec![b' '; 1_048_577]).unwrap();
        log.event("error", "CP-AFTER-ROTATION", "Persistent failure");
        assert!(d.path().join("app.1.jsonl").exists());
        assert_eq!(log.read()[0]["code"], "CP-AFTER-ROTATION");
    }
}
