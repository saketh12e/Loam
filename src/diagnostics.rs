//! Bounded diagnostic event log for recovery and spill flows.

use std::collections::VecDeque;

const MAX_EVENTS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
}

#[derive(Debug, Clone)]
pub struct Event {
    pub level: Level,
    pub message: String,
}

#[derive(Default)]
pub struct Diagnostics {
    events: VecDeque<Event>,
}

impl Diagnostics {
    pub fn info(&mut self, message: impl Into<String>) {
        self.push(Level::Info, message.into());
    }

    pub fn warn(&mut self, message: impl Into<String>) {
        self.push(Level::Warn, message.into());
    }

    fn push(&mut self, level: Level, message: String) {
        if self.events.len() == MAX_EVENTS {
            self.events.pop_front();
        }
        self.events.push_back(Event { level, message });
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        for e in &self.events {
            let tag = match e.level {
                Level::Info => "info",
                Level::Warn => "warn",
            };
            out.push_str(&format!("{tag}: {}\n", e.message));
        }
        out
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_log() {
        let mut d = Diagnostics::default();
        for i in 0..100 {
            d.info(format!("event {i}"));
        }
        assert_eq!(d.len(), MAX_EVENTS);
        assert!(d.render().contains("event 99"));
    }

    #[test]
    fn oldest_events_evicted() {
        let mut d = Diagnostics::default();
        for i in 0..80 {
            d.warn(format!("w{i}"));
        }
        assert_eq!(d.len(), MAX_EVENTS);
        assert!(d.render().contains("w79"));
        assert!(!d.render().contains("w0"));
    }
}
