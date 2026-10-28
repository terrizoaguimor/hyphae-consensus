// SPDX-License-Identifier: Apache-2.0
//! Judge-lane invite codes (owner decision: a reserved budget slice and
//! slot behind codes handed out in the submission's testing instructions).
//!
//! The credential file holds the SHA-256 of each code as 64 lowercase hex
//! characters, one per line (`#` comments and blank lines allowed). The
//! codes themselves exist only in the submission form: this module never
//! sees, stores or logs one in plain text, and a malformed line is
//! reported by its line number only, never by its content, because a
//! plain-text code pasted by mistake must not end up in a log.
//!
//! Lookup hashes the presented code and compares the digest against every
//! stored digest in constant time (every digest is visited, every byte is
//! compared), so timing reveals neither which digest matched nor how far
//! a comparison got. Revocation is editing the file and restarting, or the
//! ledger's kill switch for everything at once.
//!
//! The digests are unsalted SHA-256, so a code must be long and random:
//! `deploy/scripts/judge-codes.sh` makes them from 140 random bits (28
//! symbols of a 32-symbol alphabet of digits and capital letters), far
//! beyond an offline guess should the file leak. The live gate also bounds what one code can start per UTC
//! day (`CONSENSUS_LIVE_JUDGE_CODE_DAILY`, [`JudgeCodes::lookup`] is what
//! it counts by), so a leaked code drains the judge slice no faster than
//! that.

use sha2::{Digest, Sha256};
use std::path::Path;
use thiserror::Error;

/// Longest code accepted for hashing; longer input is refused unread.
pub const MAX_CODE_LEN: usize = 256;

#[derive(Debug, Error)]
pub enum JudgeCodesError {
    #[error("cannot read {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{path} line {line} is not a 64-character lowercase SHA-256 hex digest")]
    MalformedLine { path: String, line: usize },
    #[error("{path} holds no digests")]
    Empty { path: String },
}

/// The set of accepted code digests.
#[derive(Clone, Default)]
pub struct JudgeCodes {
    digests: Vec<[u8; 32]>,
}

impl std::fmt::Debug for JudgeCodes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JudgeCodes")
            .field("count", &self.digests.len())
            .finish()
    }
}

impl JudgeCodes {
    /// No codes: every presentation is refused and the judge lane is
    /// effectively closed.
    pub fn none() -> Self {
        Self::default()
    }

    /// Reads the digest file described in the module docs.
    pub fn load(path: &Path) -> Result<Self, JudgeCodesError> {
        let shown = path.display().to_string();
        let text = std::fs::read_to_string(path).map_err(|source| JudgeCodesError::Read {
            path: shown.clone(),
            source,
        })?;
        Self::parse(&text).map_err(|line| match line {
            0 => JudgeCodesError::Empty {
                path: shown.clone(),
            },
            line => JudgeCodesError::MalformedLine {
                path: shown.clone(),
                line,
            },
        })
    }

    /// Parses digest lines; `Err(line)` names the offending 1-based line,
    /// `Err(0)` means no digest at all.
    fn parse(text: &str) -> Result<Self, usize> {
        let mut digests = Vec::new();
        for (idx, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let bytes = line.as_bytes();
            if bytes.len() != 64 || !bytes.iter().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
                return Err(idx + 1);
            }
            let mut digest = [0u8; 32];
            for (i, pair) in bytes.chunks(2).enumerate() {
                let hex = std::str::from_utf8(pair).expect("ascii");
                digest[i] = u8::from_str_radix(hex, 16).expect("validated hex");
            }
            digests.push(digest);
        }
        if digests.is_empty() {
            return Err(0);
        }
        Ok(Self { digests })
    }

    /// Number of codes that would be accepted.
    pub fn len(&self) -> usize {
        self.digests.len()
    }

    pub fn is_empty(&self) -> bool {
        self.digests.is_empty()
    }

    /// Lowercase hex SHA-256 of a code, the form the file stores. Exposed
    /// so an operator tool can produce file lines without any other code.
    pub fn digest_hex(code: &str) -> String {
        let digest = Sha256::digest(code.trim().as_bytes());
        digest.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// True when `code` hashes to one of the stored digests. Constant time
    /// in the number of digests and their bytes; an over-long code is
    /// refused without hashing.
    pub fn verify(&self, code: &str) -> bool {
        self.lookup(code).is_some()
    }

    /// The code's id when it hashes to one of the stored digests (the same
    /// constant-time comparison as [`verify`](Self::verify)): what the live
    /// gate counts a code's daily admissions by. Two equal lines in the file
    /// are one code.
    pub fn lookup(&self, code: &str) -> Option<JudgeCodeId> {
        let code = code.trim();
        if code.is_empty() || code.len() > MAX_CODE_LEN {
            return None;
        }
        let presented: [u8; 32] = Sha256::digest(code.as_bytes()).into();
        let mut any_match = 0u8;
        for stored in &self.digests {
            let mut diff = 0u8;
            for (a, b) in stored.iter().zip(presented.iter()) {
                diff |= a ^ b;
            }
            // diff == 0 exactly when every byte matched.
            any_match |= u8::from(diff == 0);
        }
        (any_match == 1).then_some(JudgeCodeId(presented))
    }
}

/// A judge code as the gate counts it: the digest the file stores for it.
/// Never printed, like the code itself.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct JudgeCodeId([u8; 32]);

impl std::fmt::Debug for JudgeCodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("JudgeCodeId(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifies_hashed_codes_and_refuses_everything_else() {
        let file = format!(
            "# judges\n{}\n\n{}\n",
            JudgeCodes::digest_hex("alpha-7231"),
            JudgeCodes::digest_hex("bravo-9900")
        );
        let codes = JudgeCodes::parse(&file).unwrap();
        assert_eq!(codes.len(), 2);
        assert!(codes.verify("alpha-7231"));
        assert!(codes.verify("  bravo-9900\n"));
        assert!(!codes.verify("alpha-7232"));
        assert!(!codes.verify(""));
        assert!(!codes.verify(&"x".repeat(MAX_CODE_LEN + 1)));
        // The digest itself is not a code.
        assert!(!codes.verify(&JudgeCodes::digest_hex("alpha-7231")));
        assert!(!JudgeCodes::none().verify("alpha-7231"));
    }

    #[test]
    fn a_known_code_has_one_id_however_it_is_presented() {
        let file = format!(
            "{}\n{}\n{}\n",
            JudgeCodes::digest_hex("alpha-7231"),
            JudgeCodes::digest_hex("bravo-9900"),
            JudgeCodes::digest_hex("alpha-7231")
        );
        let codes = JudgeCodes::parse(&file).unwrap();
        let alpha = codes.lookup("alpha-7231").expect("a known code");
        assert_eq!(codes.lookup("  alpha-7231\n"), Some(alpha), "trimmed");
        let bravo = codes.lookup("bravo-9900").expect("a known code");
        assert_ne!(alpha, bravo, "each code is counted on its own");
        assert_eq!(codes.lookup("alpha-7232"), None);
        assert_eq!(codes.lookup(""), None);
        assert_eq!(JudgeCodes::none().lookup("alpha-7231"), None);
        assert_eq!(format!("{alpha:?}"), "JudgeCodeId(<redacted>)");
    }

    #[test]
    fn malformed_lines_are_reported_by_number_only() {
        let err = JudgeCodes::parse("# ok\nplain-text-code\n").unwrap_err();
        assert_eq!(err, 2);
        let upper = JudgeCodes::digest_hex("x").to_uppercase();
        assert_eq!(JudgeCodes::parse(&upper).unwrap_err(), 1);
        assert_eq!(JudgeCodes::parse("\n# nothing\n").unwrap_err(), 0);
        assert!(format!(
            "{:?}",
            JudgeCodes::parse(&JudgeCodes::digest_hex("c")).unwrap()
        )
        .contains("count: 1"));
    }
}
