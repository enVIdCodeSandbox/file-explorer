//! In-app passcodes for folders and drives.
//!
//! A locked path asks for its passcode before this app will list it, open
//! anything in it, show its thumbnails or return its files in a search. This is
//! a privacy screen inside the app, not encryption and not an access control:
//! other programs and the operating system still see the files normally, and
//! anyone who can read the app's folder can remove the lock. Passcodes are
//! never stored; only a salted Argon2 hash is.

use crate::{CoreError, Result};
use argon2::password_hash::{PasswordHash, SaltString};
use argon2::{Argon2, PasswordHasher, PasswordVerifier};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Lock {
    pub id: String,
    pub path: String,
    pub hash: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct LockFile {
    /// Hash of the main password, which may remove any lock.
    pub main: Option<String>,
    pub locks: Vec<Lock>,
    /// Hash of the password asked for before anything is deleted for good (when that option is on).
    pub delete_guard: Option<String>,
}

/// The locks, plus which of them are open for now.
#[derive(Default)]
pub struct Locks {
    pub file: LockFile,
    open: HashMap<String, Instant>,
}

pub fn hash(secret: &str) -> Result<String> {
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).map_err(|e| CoreError::Invalid(format!("no random numbers available: {e}")))?;
    let salt = SaltString::encode_b64(&salt).map_err(|e| CoreError::Invalid(e.to_string()))?;
    Argon2::default()
        .hash_password(secret.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| CoreError::Invalid(format!("could not protect the passcode: {e}")))
}

pub fn verify(secret: &str, phc: &str) -> bool {
    PasswordHash::new(phc).map(|h| Argon2::default().verify_password(secret.as_bytes(), &h).is_ok()).unwrap_or(false)
}

fn normal(path: &str) -> String {
    let t = path.replace('\\', "/");
    let t = t.trim_end_matches('/');
    let t = if t.is_empty() { "/" } else { t };
    // Windows paths are not case sensitive.
    if t.len() > 1 && t.as_bytes()[1] == b':' || t.starts_with("//") { t.to_lowercase() } else { t.to_string() }
}

/// Is `path` the locked path itself or inside it?
fn covers(lock: &str, path: &str) -> bool {
    let (l, p) = (normal(lock), normal(path));
    p == l || (p.starts_with(&l) && (l == "/" || p.as_bytes().get(l.len()) == Some(&b'/')))
}

impl Locks {
    pub fn load(file: &Path) -> Locks {
        let parsed = std::fs::read(file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Locks { file: parsed, open: HashMap::new() }
    }

    pub fn save(&self, file: &Path) -> Result<()> {
        crate::settings::write_json(file, &self.file)
    }

    pub fn is_empty(&self) -> bool {
        self.file.locks.is_empty()
    }

    /// The lock that covers `path` and is not currently open. `relock` is how
    /// long an unlock lasts (`None`: until the app closes).
    pub fn blocking(&self, path: &str, relock: Option<Duration>) -> Option<&Lock> {
        // The deepest (most specific) lock decides.
        self.file
            .locks
            .iter()
            .filter(|l| covers(&l.path, path))
            .max_by_key(|l| normal(&l.path).len())
            .filter(|l| match self.open.get(&l.id) {
                Some(at) => relock.is_some_and(|r| at.elapsed() > r),
                None => true,
            })
    }

    pub fn lock_for(&self, path: &str) -> Option<&Lock> {
        self.file.locks.iter().filter(|l| covers(&l.path, path)).max_by_key(|l| normal(&l.path).len())
    }

    pub fn add(&mut self, path: &str, passcode: &str) -> Result<String> {
        if passcode.chars().count() < 4 {
            return Err(CoreError::Invalid("Use at least 4 characters.".into()));
        }
        if self.file.locks.iter().any(|l| normal(&l.path) == normal(path)) {
            return Err(CoreError::Invalid("That item is already locked.".into()));
        }
        let mut id = [0u8; 8];
        getrandom::fill(&mut id).map_err(|e| CoreError::Invalid(e.to_string()))?;
        let id: String = id.iter().map(|b| format!("{b:02x}")).collect();
        self.file.locks.push(Lock { id: id.clone(), path: path.to_string(), hash: hash(passcode)? });
        self.open.insert(id.clone(), Instant::now());
        Ok(id)
    }

    /// Try a passcode for the lock covering `path`; opens it on success.
    pub fn unlock(&mut self, path: &str, passcode: &str) -> bool {
        let Some(lock) = self.lock_for(path).cloned() else { return true };
        if verify(passcode, &lock.hash) {
            self.open.insert(lock.id, Instant::now());
            true
        } else {
            false
        }
    }

    /// Close every lock again.
    pub fn relock_all(&mut self) {
        self.open.clear();
    }

    /// Remove a lock. With a main password set, that is what authorises it;
    /// without one, the lock's own passcode is.
    pub fn remove(&mut self, id: &str, secret: &str) -> Result<()> {
        let lock = self.file.locks.iter().find(|l| l.id == id).ok_or_else(|| CoreError::Invalid("No such lock.".into()))?;
        let ok = match &self.file.main {
            Some(main) => verify(secret, main),
            None => verify(secret, &lock.hash),
        };
        if !ok {
            return Err(CoreError::Invalid("That is not the right password.".into()));
        }
        self.file.locks.retain(|l| l.id != id);
        self.open.remove(id);
        Ok(())
    }

    /// Set or change the main password. When one exists, `old` must match it.
    pub fn set_main(&mut self, old: &str, new: &str) -> Result<()> {
        if let Some(main) = &self.file.main {
            if !verify(old, main) {
                return Err(CoreError::Invalid("The current main password is not right.".into()));
            }
        }
        if new.chars().count() < 6 {
            return Err(CoreError::Invalid("The main password needs at least 6 characters.".into()));
        }
        self.file.main = Some(hash(new)?);
        Ok(())
    }

    /// Set, change or (with an empty `new`) remove the password for deleting for good. When one exists, `old` must match it.
    pub fn set_delete_guard(&mut self, old: &str, new: &str) -> Result<()> {
        if let Some(current) = &self.file.delete_guard {
            if !verify(old, current) {
                return Err(CoreError::Invalid("The current password is not right.".into()));
            }
        }
        if new.is_empty() {
            self.file.delete_guard = None;
            return Ok(());
        }
        if new.chars().count() < 4 {
            return Err(CoreError::Invalid("The password needs at least 4 characters.".into()));
        }
        self.file.delete_guard = Some(hash(new)?);
        Ok(())
    }

    /// Is `secret` the password for deleting for good? (False when none is set.)
    pub fn check_delete_guard(&self, secret: &str) -> bool {
        self.file.delete_guard.as_deref().is_some_and(|h| verify(secret, h))
    }

    pub fn is_open(&self, id: &str, relock: Option<Duration>) -> bool {
        self.open.get(id).is_some_and(|at| relock.is_none_or(|r| at.elapsed() <= r))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_password_is_set_checked_changed_and_removed() {
        let mut l = Locks::default();
        assert!(!l.check_delete_guard("anything"));
        assert!(l.set_delete_guard("", "abc").is_err());
        l.set_delete_guard("", "sesame").unwrap();
        assert!(l.check_delete_guard("sesame") && !l.check_delete_guard("sesamE"));
        assert!(l.set_delete_guard("wrong", "other1").is_err());
        l.set_delete_guard("sesame", "other1").unwrap();
        assert!(l.check_delete_guard("other1") && !l.check_delete_guard("sesame"));
        l.set_delete_guard("other1", "").unwrap();
        assert!(l.file.delete_guard.is_none());
    }

    #[test]
    fn covers_the_folder_and_everything_inside_it() {
        assert!(covers("/home/me/Private", "/home/me/Private"));
        assert!(covers("/home/me/Private", "/home/me/Private/a/b.txt"));
        assert!(covers("/home/me/Private/", "/home/me/Private/a"));
        assert!(!covers("/home/me/Private", "/home/me/Private2"), "a longer name is not inside");
        assert!(!covers("/home/me/Private", "/home/me"));
        assert!(covers("/", "/anything"));
    }

    #[test]
    fn windows_paths_ignore_case_and_slash_direction() {
        assert!(covers(r"D:\Secret", "d:/secret/x.txt"));
        assert!(covers(r"\\nas\Share", r"\\NAS\share\f"));
        assert!(!covers(r"D:\Secret", r"D:\Secrets"));
    }

    #[test]
    fn a_passcode_unlocks_and_a_wrong_one_does_not() {
        let mut l = Locks::default();
        let id = l.add("/p", "hunter22").unwrap();
        assert!(l.is_open(&id, None), "just created, so open for now");
        l.relock_all();
        assert!(l.blocking("/p/x", None).is_some());
        assert!(!l.unlock("/p/x", "wrong"));
        assert!(l.blocking("/p/x", None).is_some());
        assert!(l.unlock("/p/x", "hunter22"));
        assert!(l.blocking("/p/x", None).is_none());
        assert!(l.blocking("/elsewhere", None).is_none());
    }

    #[test]
    fn an_unlock_expires() {
        let mut l = Locks::default();
        l.add("/p", "hunter22").unwrap();
        assert!(l.blocking("/p", Some(Duration::from_secs(60))).is_none());
        assert!(l.blocking("/p", Some(Duration::ZERO)).is_some() || l.blocking("/p", Some(Duration::ZERO)).is_none());
        std::thread::sleep(Duration::from_millis(5));
        assert!(l.blocking("/p", Some(Duration::from_millis(1))).is_some());
    }

    #[test]
    fn the_deepest_lock_decides() {
        let mut l = Locks::default();
        l.add("/a", "aaaaaa").unwrap();
        l.add("/a/b", "bbbbbb").unwrap();
        l.relock_all();
        assert!(l.unlock("/a/b/c", "bbbbbb"));
        assert!(l.blocking("/a/b/c", None).is_none());
        assert!(l.blocking("/a/other", None).is_some(), "the outer lock is still closed");
    }

    #[test]
    fn removing_needs_the_main_password_when_there_is_one() {
        let mut l = Locks::default();
        let id = l.add("/p", "hunter22").unwrap();
        assert!(l.remove(&id, "nope").is_err());
        l.set_main("", "mainpass1").unwrap();
        assert!(l.remove(&id, "hunter22").is_err(), "the lock's own passcode no longer removes it");
        assert!(l.remove(&id, "mainpass1").is_ok());
        assert!(l.is_empty());
    }

    #[test]
    fn changing_the_main_password_needs_the_old_one() {
        let mut l = Locks::default();
        l.set_main("", "mainpass1").unwrap();
        assert!(l.set_main("wrong", "another1").is_err());
        assert!(l.set_main("mainpass1", "another1").is_ok());
        assert!(l.set_main("another1", "short").is_err());
    }

    #[test]
    fn short_passcodes_and_duplicates_are_refused() {
        let mut l = Locks::default();
        assert!(l.add("/p", "abc").is_err());
        l.add("/p", "abcd").unwrap();
        assert!(l.add("/p/", "abcd").is_err());
    }

    #[test]
    fn survives_a_restart_without_storing_the_passcode() {
        let dir = std::env::temp_dir().join(format!("fast-locks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("locks.json");
        let mut l = Locks::default();
        l.add("/p", "hunter22").unwrap();
        l.save(&file).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(!text.contains("hunter22"));
        let mut again = Locks::load(&file);
        assert!(again.blocking("/p", None).is_some(), "a fresh start is locked");
        assert!(again.unlock("/p", "hunter22"));
    }
}
