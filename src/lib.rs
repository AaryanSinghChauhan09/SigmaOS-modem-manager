// SPDX-License-Identifier: Apache-2.0
//! SigmaOS-omarchy-mm2: omarchy mm2 component model.
//!
//! Real implementation: omarchy_mms, events, validation, deterministic storage.
//! Replaces the placeholder stub.

use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OmarchyMm {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub id: String,
    pub subject: String,
    pub detail: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OmarchyMm2Error {
    EmptyOmarchyMmName,
    DuplicateOmarchyMm(String),
    EmptyEventSubject,
}

impl fmt::Display for OmarchyMm2Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyOmarchyMmName => write!(f, "omarchy_mm name must not be empty"),
            Self::DuplicateOmarchyMm(id) => write!(f, "duplicate omarchy_mm {:?}", id),
            Self::EmptyEventSubject => write!(f, "event subject must not be empty"),
        }
    }
}

impl std::error::Error for OmarchyMm2Error {}

pub struct OmarchyMm2 {
    pub name: String,
    omarchy_mms: BTreeMap<String, OmarchyMm>,
    events: BTreeMap<String, Event>,
}

impl OmarchyMm2 {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            omarchy_mms: BTreeMap::new(),
            events: BTreeMap::new(),
        }
    }

    pub fn add_omarchy_mm(&mut self, item: OmarchyMm) -> Result<(), OmarchyMm2Error> {
        if item.name.trim().is_empty() {
            return Err(OmarchyMm2Error::EmptyOmarchyMmName);
        }
        let id = item.id.clone();
        if self.omarchy_mms.contains_key(&id) {
            return Err(OmarchyMm2Error::DuplicateOmarchyMm(id.clone()));
        }
        self.omarchy_mms.insert(id, item);
        Ok(())
    }

    pub fn omarchy_mm_count(&self) -> usize {
        self.omarchy_mms.len()
    }

    pub fn get_omarchy_mm(&self, id: &str) -> Option<&OmarchyMm> {
        self.omarchy_mms.get(id)
    }

    pub fn add_event(&mut self, event: Event) -> Result<(), OmarchyMm2Error> {
        if event.subject.trim().is_empty() {
            return Err(OmarchyMm2Error::EmptyEventSubject);
        }
        self.events.insert(event.id.clone(), event);
        Ok(())
    }

    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    pub fn get_event(&self, id: &str) -> Option<&Event> {
        self.events.get(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_omarchy_mm(id: &str, name: &str) -> OmarchyMm {
        OmarchyMm {
            id: id.into(),
            name: name.into(),
            kind: "default".into(),
            detail: String::new(),
        }
    }

    #[test]
    fn test_create() {
        let m = OmarchyMm2::new("OmarchyMm2");
        assert_eq!(m.name, "OmarchyMm2");
        assert_eq!(m.omarchy_mm_count(), 0);
        assert_eq!(m.event_count(), 0);
    }

    #[test]
    fn test_add_omarchy_mm() {
        let mut m = OmarchyMm2::new("OmarchyMm2");
        m.add_omarchy_mm(sample_omarchy_mm("a1", "Alpha")).unwrap();
        assert_eq!(m.omarchy_mm_count(), 1);
        assert!(m.get_omarchy_mm("a1").is_some());
    }

    #[test]
    fn test_empty_name_rejected() {
        let mut m = OmarchyMm2::new("OmarchyMm2");
        assert!(m.add_omarchy_mm(sample_omarchy_mm("x", "   ")).is_err());
    }

    #[test]
    fn test_duplicate_rejected() {
        let mut m = OmarchyMm2::new("OmarchyMm2");
        m.add_omarchy_mm(sample_omarchy_mm("x", "One")).unwrap();
        assert!(m.add_omarchy_mm(sample_omarchy_mm("x", "Two")).is_err());
    }

    #[test]
    fn test_event_validation() {
        let mut m = OmarchyMm2::new("OmarchyMm2");
        let bad = Event { id: "e1".into(), subject: "".into(), detail: String::new(), timestamp: 0 };
        assert!(m.add_event(bad).is_err());
        let good = Event { id: "e1".into(), subject: "boot".into(), detail: String::new(), timestamp: 1 };
        m.add_event(good).unwrap();
        assert_eq!(m.event_count(), 1);
    }
}
