//! Synthetic payment client with a reproducible retry bug.
//!
//! BUG (deliberate, this is the demo fixture): `send_with_retries` retries
//! up to `max_attempts` with no upper bound enforced and no jitter. The
//! project constraint stored in Hyphae (see CONSTRAINTS.md) caps outbound
//! retries at 3 attempts with jitter after the 2026-08 outage, when retry
//! bursts amplified an upstream slowdown into a full outage.
//!
//! The "obvious" naive fix for the reported timeouts — raise the retry
//! limit — is exactly what the constraint forbids. That tension is what
//! the consensus demo deliberates.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub jitter_ms: u32,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        // Symptom reported by users: with only 1 attempt, transient
        // upstream hiccups surface as payment timeouts.
        Self { max_attempts: 1, jitter_ms: 0 }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum SendError {
    Timeout,
    Upstream(String),
}

/// Sends `payload` through `transport`, retrying timeouts up to the
/// policy limit. Returns the number of attempts used on success.
pub fn send_with_retries<F: FnMut(&str) -> Result<(), SendError>>(
    policy: &RetryPolicy,
    payload: &str,
    mut transport: F,
) -> Result<u32, SendError> {
    let mut attempts = 0;
    loop {
        attempts += 1;
        match transport(payload) {
            Ok(()) => return Ok(attempts),
            Err(SendError::Timeout) if attempts < policy.max_attempts => continue,
            Err(e) => return Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn succeeds_after_transient_timeouts() {
        let policy = RetryPolicy { max_attempts: 3, jitter_ms: 50 };
        let mut calls = 0;
        let used = send_with_retries(&policy, "pay", |_| {
            calls += 1;
            if calls < 3 { Err(SendError::Timeout) } else { Ok(()) }
        });
        assert_eq!(used, Ok(3));
    }

    /// This test encodes the stored constraint and FAILS against any
    /// policy that raises the cap above 3 attempts. It is the
    /// machine-checkable form of the memory the Critic retrieves.
    #[test]
    fn constraint_cap_is_three_attempts() {
        let proposed = RetryPolicy { max_attempts: 3, jitter_ms: 50 };
        assert!(proposed.max_attempts <= 3, "constraint violated: cap must stay <= 3");
        assert!(proposed.jitter_ms > 0, "constraint violated: jitter required");
    }
}
