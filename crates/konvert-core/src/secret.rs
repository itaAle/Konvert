//! Passwords typed by the user: kept in memory only, never printed, logged
//! or serialized, and wiped when dropped.
//!
//! The wiping covers Konvert's own copies. Libraries that read a password
//! (lopdf normalizes it before hashing) may hold short-lived copies of their
//! own, freed when they are done.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{Ordering, compiler_fence};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::CoreError;

/// A password.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Secret(Vec<u8>);

impl Secret {
    pub fn new(text: String) -> Self {
        Self(text.into_bytes())
    }

    /// The password, for the code that needs it (never to print it).
    pub fn expose(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or_default()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// A password nobody knows: `length` random letters and digits.
    pub fn random(length: usize) -> Result<Self, CoreError> {
        const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789";
        let mut bytes = random_bytes(length)?;
        for byte in bytes.iter_mut() {
            *byte = ALPHABET[usize::from(*byte) % ALPHABET.len()];
        }
        Ok(Self(bytes))
    }
}

impl From<&str> for Secret {
    fn from(text: &str) -> Self {
        Self(text.as_bytes().to_vec())
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}

/// Never written out: a request sent back keeps no password.
impl Serialize for Secret {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str("")
    }
}

impl<'de> Deserialize<'de> for Secret {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(Secret::new)
    }
}

/// Overwrites `bytes` with zeros, in a way the compiler keeps.
pub fn wipe(bytes: &mut [u8]) {
    bytes.fill(0);
    std::hint::black_box(&*bytes);
    compiler_fence(Ordering::SeqCst);
}

/// Random bytes from the operating system.
pub fn random_bytes(length: usize) -> Result<Vec<u8>, CoreError> {
    let mut bytes = vec![0; length];
    getrandom::fill(&mut bytes).map_err(|e| CoreError::Io(std::io::Error::other(e.to_string())))?;
    Ok(bytes)
}

/// Passwords that open some PDF files, by path.
#[derive(Clone, Default)]
pub struct Passwords(HashMap<PathBuf, Secret>);

impl Passwords {
    pub fn insert(&mut self, path: PathBuf, password: Secret) {
        self.0.insert(path, password);
    }

    pub fn get(&self, path: &Path) -> Option<&Secret> {
        self.0.get(path)
    }

    pub fn remove(&mut self, path: &Path) {
        self.0.remove(path);
    }

    /// The passwords of `paths` only.
    pub fn of<'a>(&self, paths: impl IntoIterator<Item = &'a Path>) -> Self {
        Self(
            paths
                .into_iter()
                .filter_map(|p| Some((p.to_path_buf(), self.0.get(p)?.clone())))
                .collect(),
        )
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Passwords {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Passwords({})", self.0.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passwords_are_never_printed_nor_written_out() {
        let secret = Secret::from("segreta");
        assert_eq!(secret.expose(), "segreta");
        assert_eq!(format!("{secret:?}"), "Secret(***)");
        assert_eq!(serde_json::to_string(&secret).unwrap(), r#""""#);
        let read: Secret = serde_json::from_str(r#""àbc""#).unwrap();
        assert_eq!(read.expose(), "àbc");
        let mut passwords = Passwords::default();
        passwords.insert("a.pdf".into(), secret);
        assert_eq!(format!("{passwords:?}"), "Passwords(1)");
        assert_eq!(passwords.of([Path::new("b.pdf")]).len(), 0);
        assert_eq!(
            passwords
                .of([Path::new("a.pdf")])
                .get(Path::new("a.pdf"))
                .unwrap()
                .expose(),
            "segreta"
        );
    }

    #[test]
    fn wiped_bytes_are_zero_and_random_ones_differ() {
        let mut bytes = b"segreta".to_vec();
        wipe(&mut bytes);
        assert!(bytes.iter().all(|&b| b == 0));
        let (a, b) = (Secret::random(32).unwrap(), Secret::random(32).unwrap());
        assert_eq!(a.expose().len(), 32);
        assert!(a.expose().chars().all(|c| c.is_ascii_alphanumeric()));
        assert_ne!(a, b);
    }
}
