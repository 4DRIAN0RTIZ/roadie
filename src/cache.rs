//! Persistent cache of repo summaries and board issues.
//!
//! Pure data (`Cache`) plus small IO helpers. Paths and clocks are injected so
//! everything is unit-testable; writes are atomic (temp file + rename) and a
//! missing, corrupt or old-schema file is silently ignored.
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::thread;

use serde::{Deserialize, Serialize};

use crate::domain::{Issue, RepoSummary};

/// Bump when the on-disk layout changes; other versions are ignored.
pub const SCHEMA: u32 = 2;
pub const FILE_NAME: &str = "cache.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stamped<T> {
    /// Unix seconds when the data was fetched.
    pub at: u64,
    pub value: T,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Board {
    pub issues: Vec<Issue>,
    pub milestones: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Entry {
    pub summary: Option<Stamped<RepoSummary>>,
    pub board: Option<Stamped<Board>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cache {
    pub version: u32,
    pub repos: BTreeMap<String, Entry>,
}

impl Default for Cache {
    fn default() -> Self {
        Self {
            version: SCHEMA,
            repos: BTreeMap::new(),
        }
    }
}

impl Cache {
    /// Reads the cache; anything unreadable or of another schema is empty.
    pub fn load(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|s| Self::from_json(&s))
            .unwrap_or_default()
    }

    pub fn from_json(s: &str) -> Option<Self> {
        let c: Cache = serde_json::from_str(s).ok()?;
        (c.version == SCHEMA).then_some(c)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn summary(&self, repo: &str) -> Option<&Stamped<RepoSummary>> {
        self.repos.get(repo)?.summary.as_ref()
    }

    pub fn board(&self, repo: &str) -> Option<&Stamped<Board>> {
        self.repos.get(repo)?.board.as_ref()
    }

    pub fn put_summary(&mut self, repo: &str, s: RepoSummary, now: u64) {
        self.repos.entry(repo.to_string()).or_default().summary =
            Some(Stamped { at: now, value: s });
    }

    pub fn put_board(&mut self, repo: &str, issues: Vec<Issue>, milestones: Vec<String>, now: u64) {
        self.repos.entry(repo.to_string()).or_default().board = Some(Stamped {
            at: now,
            value: Board { issues, milestones },
        });
    }

    pub fn drop_board(&mut self, repo: &str) {
        if let Some(e) = self.repos.get_mut(repo) {
            e.board = None;
        }
    }

    /// Forgets everything cached for `repo` (after a mutation that could not
    /// be reflected).
    pub fn invalidate(&mut self, repo: &str) {
        self.repos.remove(repo);
    }
}

/// `1m ago`-style age of a timestamp.
pub fn age_text(now: u64, at: u64) -> String {
    let d = now.saturating_sub(at);
    match d {
        0..=59 => "just now".into(),
        60..=3599 => format!("{}m ago", d / 60),
        3600..=86399 => format!("{}h ago", d / 3600),
        _ => format!("{}d ago", d / 86400),
    }
}

pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Cache file location: `ROADMAP_CACHE_DIR`, else `XDG_CACHE_HOME/roadie`,
/// else `~/.cache/roadie`. `env` is injected for tests.
pub fn default_path(env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let non_empty = |k: &str| env(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let dir = non_empty("ROADMAP_CACHE_DIR")
        .or_else(|| non_empty("XDG_CACHE_HOME").map(|d| d.join("roadie")))
        .or_else(|| non_empty("HOME").map(|h| h.join(".cache").join("roadie")))?;
    Some(dir.join(FILE_NAME))
}

/// Writes `contents` to `path` via a temp file and rename, creating parents.
pub fn write_atomic(path: &Path, contents: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".{}.tmp", std::process::id()));
    let tmp = PathBuf::from(tmp);
    let res = fs::write(&tmp, contents).and_then(|_| fs::rename(&tmp, path));
    if res.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    res
}

/// Background writer: the UI thread only sends a serialized snapshot; the
/// thread coalesces bursts and writes the latest one atomically.
pub fn spawn_writer(path: PathBuf) -> Sender<String> {
    let (tx, rx) = channel::<String>();
    thread::spawn(move || {
        while let Ok(mut latest) = rx.recv() {
            while let Ok(newer) = rx.try_recv() {
                latest = newer;
            }
            let _ = write_atomic(&path, &latest);
        }
    });
    tx
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Temp dir removed on drop.
    struct TmpDir(PathBuf);

    impl std::ops::Deref for TmpDir {
        type Target = PathBuf;
        fn deref(&self) -> &PathBuf {
            &self.0
        }
    }

    impl Drop for TmpDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn tmp_dir(name: &str) -> TmpDir {
        let d = std::env::temp_dir().join(format!("roadie-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        TmpDir(d)
    }

    fn sample() -> Cache {
        let mut c = Cache::default();
        c.put_summary(
            "a",
            RepoSummary {
                description: "d".into(),
                counts: [1, 2, 3, 4],
                active: Some("x".into()),
                latest_release: None,
            },
            100,
        );
        c.put_board(
            "a",
            vec![Issue {
                number: 7,
                title: "[roadmap] T".into(),
                ..Default::default()
            }],
            vec!["M1".into()],
            110,
        );
        c
    }

    #[test]
    fn roundtrip_through_disk() {
        let dir = tmp_dir("roundtrip");
        let path = dir.join("sub").join(FILE_NAME);
        let c = sample();
        write_atomic(&path, &c.to_json()).unwrap();
        assert_eq!(Cache::load(&path), c);
        assert_eq!(
            Cache::load(&path).board("a").unwrap().value.issues[0].number,
            7
        );
        assert_eq!(Cache::load(&path).summary("a").unwrap().at, 100);
    }

    #[test]
    fn missing_corrupt_and_old_schema_are_ignored() {
        let dir = tmp_dir("bad");
        let path = dir.join(FILE_NAME);
        assert_eq!(Cache::load(&path), Cache::default()); // missing
        fs::write(&path, "{ not json").unwrap();
        assert_eq!(Cache::load(&path), Cache::default()); // corrupt
        fs::write(&path, "").unwrap();
        assert_eq!(Cache::load(&path), Cache::default()); // empty
        let mut old = sample();
        old.version = SCHEMA + 1;
        fs::write(&path, old.to_json()).unwrap();
        assert_eq!(Cache::load(&path), Cache::default()); // other schema
        fs::write(&path, r#"{"repos":{}}"#).unwrap();
        assert_eq!(Cache::load(&path), Cache::default()); // no version
    }

    #[test]
    fn atomic_write_replaces_and_leaves_no_temp_files() {
        let dir = tmp_dir("atomic");
        let path = dir.join(FILE_NAME);
        write_atomic(&path, "one").unwrap();
        write_atomic(&path, "two").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "two");
        let names: Vec<_> = fs::read_dir(&*dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![OsString::from(FILE_NAME)]);
        // A failing write (target is a directory) cleans its temp file.
        let blocked = dir.join("blocked");
        fs::create_dir(&blocked).unwrap();
        assert!(write_atomic(&blocked, "x").is_err());
        assert_eq!(fs::read_dir(&*dir).unwrap().count(), 2);
    }

    #[test]
    fn writer_thread_persists_latest_snapshot() {
        let dir = tmp_dir("writer");
        let path = dir.join(FILE_NAME);
        let tx = spawn_writer(path.clone());
        tx.send("first".into()).unwrap();
        tx.send(sample().to_json()).unwrap();
        let end = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while Cache::load(&path) != sample() && std::time::Instant::now() < end {
            thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(Cache::load(&path), sample());
    }

    #[test]
    fn invalidate_and_drop_board() {
        let mut c = sample();
        c.drop_board("a");
        assert!(c.board("a").is_none() && c.summary("a").is_some());
        c.invalidate("a");
        assert!(c.summary("a").is_none());
        c.invalidate("nope"); // no panic
    }

    #[test]
    fn age_text_buckets() {
        assert_eq!(age_text(100, 100), "just now");
        assert_eq!(age_text(100, 200), "just now"); // clock skew
        assert_eq!(age_text(160, 100), "1m ago");
        assert_eq!(age_text(100 + 3 * 3600 + 5, 100), "3h ago");
        assert_eq!(age_text(100 + 2 * 86400, 100), "2d ago");
    }

    #[test]
    fn path_resolution_order() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| {
                pairs
                    .iter()
                    .find(|(n, _)| *n == k)
                    .map(|(_, v)| OsString::from(v))
            }
        };
        let all = env(&[
            ("ROADMAP_CACHE_DIR", "/o"),
            ("XDG_CACHE_HOME", "/x"),
            ("HOME", "/h"),
        ]);
        assert_eq!(default_path(all), Some(PathBuf::from("/o/cache.json")));
        let xdg = env(&[("XDG_CACHE_HOME", "/x"), ("HOME", "/h")]);
        assert_eq!(
            default_path(xdg),
            Some(PathBuf::from("/x/roadie/cache.json"))
        );
        let home = env(&[("HOME", "/h"), ("XDG_CACHE_HOME", "")]);
        assert_eq!(
            default_path(home),
            Some(PathBuf::from("/h/.cache/roadie/cache.json"))
        );
        assert_eq!(default_path(env(&[])), None);
    }
}
