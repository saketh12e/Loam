//! Global dictionaries: word stores shared across columns with the same
//! value domain (for example, region codes reused by many tables).

use std::collections::HashMap;

use crate::dict::WordStore;
use crate::error::{Error, Result};

/// A process-wide dictionary registry keyed by domain name.
#[derive(Default)]
pub struct GlobalDicts {
    domains: HashMap<String, WordStore>,
}

impl GlobalDicts {
    /// Install a domain's words; replaces any previous store for the domain.
    pub fn install(&mut self, domain: &str, words: Vec<Vec<u8>>) {
        self.domains
            .insert(domain.to_string(), WordStore::build(&words));
    }

    pub fn get(&self, domain: &str) -> Result<&WordStore> {
        self.domains.get(domain).ok_or(Error::View {
            detail: format!("no domain '{domain}'"),
        })
    }

    /// Word id for a value in a domain, if present.
    pub fn lookup(&self, domain: &str, value: &[u8]) -> Option<u32> {
        let store = self.domains.get(domain)?;
        (0..store.words() as u32).find(|&i| store.word(i) == value)
    }

    pub fn domains(&self) -> usize {
        self.domains.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_and_lookup() {
        let mut g = GlobalDicts::default();
        g.install("region", vec![b"us".to_vec(), b"eu".to_vec()]);
        assert_eq!(g.lookup("region", b"eu"), Some(1));
        assert_eq!(g.lookup("region", b"ap"), None);
        assert!(g.get("missing").is_err());
        assert_eq!(g.domains(), 1);
    }
}
