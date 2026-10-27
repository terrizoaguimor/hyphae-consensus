// SPDX-License-Identifier: Apache-2.0
//! Time and identifier generation, injected so tests are deterministic.

pub trait Clock {
    /// Current time as an RFC 3339 string.
    fn now(&self) -> String;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> String {
        chrono::Utc::now().to_rfc3339()
    }
}

pub struct FixedClock(pub String);

impl FixedClock {
    pub fn new(ts: impl Into<String>) -> Self {
        Self(ts.into())
    }
}

impl Clock for FixedClock {
    fn now(&self) -> String {
        self.0.clone()
    }
}

pub trait IdGen {
    fn next(&mut self, prefix: &str) -> String;
}

pub struct UuidIdGen;

impl IdGen for UuidIdGen {
    fn next(&mut self, prefix: &str) -> String {
        format!("{prefix}-{}", uuid::Uuid::new_v4())
    }
}

/// Deterministic generator for tests and reproducible demos.
pub struct SequentialIdGen {
    counter: u64,
}

impl SequentialIdGen {
    pub fn new() -> Self {
        Self { counter: 0 }
    }
}

impl Default for SequentialIdGen {
    fn default() -> Self {
        Self::new()
    }
}

impl IdGen for SequentialIdGen {
    fn next(&mut self, prefix: &str) -> String {
        self.counter += 1;
        format!("{prefix}-{:04}", self.counter)
    }
}
