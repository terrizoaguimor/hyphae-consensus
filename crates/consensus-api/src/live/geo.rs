// SPDX-License-Identifier: Apache-2.0
//! Geographic policy for the public portal (owner decision, 2026-10-27).
//!
//! Two lists of ISO 3166-1 alpha-2 country codes:
//!
//! - **portal-blocked**: Meta's Restricted Territories at country level
//!   plus all of Ukraine (Crimea, Donetsk and Luhansk cannot be isolated
//!   by country code). These block the *whole* portal, live mode included.
//! - **live-only blocked**: countries where the model is unavailable
//!   (South Korea by default). These block live mode and nothing else, so
//!   guided and showcase stay reachable there.
//!
//! The country comes from Cloudflare's `CF-IPCountry` header. `XX`
//! (unknown), `T1` (Tor), a missing header and anything that is not two
//! ASCII letters all count as *unknown*: unknown is refused on the live
//! surface (an owner key is at stake) and allowed on the portal surface
//! (nothing is spent there). The Cloudflare edge rule is the primary
//! control for the portal-wide block; this module is the origin's own
//! check of the same list, so a request that reaches the origin without
//! passing the edge rule (or with the edge rule misconfigured) is still
//! refused. Both are derived from the same configured list.
//!
//! A block maps to HTTP 451 ([`HTTP_STATUS_BLOCKED`]) with one of two
//! fixed public message codes ([`GeoBlockCode`]); the country itself is
//! never echoed back.

use std::collections::BTreeSet;
use std::fmt;
use thiserror::Error;

/// HTTP status for a geographic block ("Unavailable For Legal Reasons").
pub const HTTP_STATUS_BLOCKED: u16 = 451;

/// Default portal-wide block: Meta's Restricted Territories at country
/// level, plus all of Ukraine (`UA`) because its occupied regions cannot
/// be isolated.
pub const DEFAULT_PORTAL_BLOCKED: &str =
    "AF BY KH CN HK MO CU ER ET HT IR IQ LY MM NI KP PK RU SO SY UA VE EH";
/// Default live-only block: South Korea (model unavailable there).
pub const DEFAULT_LIVE_ONLY_BLOCKED: &str = "KR";

/// A two-letter ISO 3166-1 alpha-2 code, stored uppercase.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CountryCode([u8; 2]);

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("not an ISO 3166-1 alpha-2 country code (two ASCII letters)")]
pub struct InvalidCountryCode;

impl CountryCode {
    /// Accepts exactly two ASCII letters in either case. `XX` and `T1` are
    /// not codes of countries: `parse` refuses `T1` (a digit) and the policy
    /// treats `XX` as unknown explicitly.
    pub fn parse(text: &str) -> Result<Self, InvalidCountryCode> {
        let bytes = text.trim().as_bytes();
        match bytes {
            [a, b] if a.is_ascii_alphabetic() && b.is_ascii_alphabetic() => Ok(CountryCode([
                a.to_ascii_uppercase(),
                b.to_ascii_uppercase(),
            ])),
            _ => Err(InvalidCountryCode),
        }
    }

    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).expect("ASCII letters")
    }

    /// Cloudflare's placeholder for an unknown location.
    fn is_unknown_placeholder(self) -> bool {
        self.0 == *b"XX"
    }
}

impl fmt::Debug for CountryCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl fmt::Display for CountryCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Parses a space- or comma-separated list of codes; duplicates collapse.
/// An empty list is an error: the operator must name the codes.
pub fn parse_country_list(text: &str) -> Result<BTreeSet<CountryCode>, GeoConfigError> {
    let mut set = BTreeSet::new();
    for item in text.split([' ', ',', '\t', '\n']).filter(|s| !s.is_empty()) {
        let code = CountryCode::parse(item).map_err(|_| GeoConfigError::InvalidCode {
            item: item.to_string(),
        })?;
        if code.is_unknown_placeholder() {
            return Err(GeoConfigError::InvalidCode {
                item: item.to_string(),
            });
        }
        set.insert(code);
    }
    if set.is_empty() {
        return Err(GeoConfigError::Empty);
    }
    Ok(set)
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum GeoConfigError {
    #[error("{item:?} is not an ISO 3166-1 alpha-2 country code")]
    InvalidCode { item: String },
    #[error("the list is empty; unset the variable to use the default")]
    Empty,
}

/// What the request is trying to reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// Any portal page or API route that spends nothing.
    Portal,
    /// Creating a live run on the owner's key.
    Live,
}

/// Fixed public message codes; the UI localizes from them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeoBlockCode {
    /// The whole portal is unavailable from this country.
    GeoBlockedPortal,
    /// Live mode is unavailable from this country or an unknown one.
    GeoBlockedLive,
}

impl GeoBlockCode {
    pub fn as_str(self) -> &'static str {
        match self {
            GeoBlockCode::GeoBlockedPortal => "geo_blocked_portal",
            GeoBlockCode::GeoBlockedLive => "geo_blocked_live",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeoDecision {
    Allow,
    Block {
        code: GeoBlockCode,
        /// Operator-facing reason (names the rule, not the visitor).
        reason: &'static str,
    },
}

impl GeoDecision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, GeoDecision::Allow)
    }

    /// HTTP status to answer with: 451 for a block, none for allow.
    pub fn http_status(&self) -> Option<u16> {
        match self {
            GeoDecision::Allow => None,
            GeoDecision::Block { .. } => Some(HTTP_STATUS_BLOCKED),
        }
    }
}

/// The configured lists; see the module docs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeoPolicy {
    portal_blocked: BTreeSet<CountryCode>,
    live_only_blocked: BTreeSet<CountryCode>,
}

impl GeoPolicy {
    pub fn new(
        portal_blocked: BTreeSet<CountryCode>,
        live_only_blocked: BTreeSet<CountryCode>,
    ) -> Self {
        Self {
            portal_blocked,
            live_only_blocked,
        }
    }

    /// The owner's 2026-10-27 lists.
    pub fn default_policy() -> Self {
        Self::new(
            parse_country_list(DEFAULT_PORTAL_BLOCKED).expect("default list parses"),
            parse_country_list(DEFAULT_LIVE_ONLY_BLOCKED).expect("default list parses"),
        )
    }

    pub fn portal_blocked(&self) -> &BTreeSet<CountryCode> {
        &self.portal_blocked
    }

    pub fn live_only_blocked(&self) -> &BTreeSet<CountryCode> {
        &self.live_only_blocked
    }

    /// Decides for the raw `CF-IPCountry` value (`None` when the header is
    /// absent). Lowercase input is normalized; `XX`, `T1` and anything that
    /// is not two letters are unknown.
    pub fn decide(&self, country: Option<&str>, surface: Surface) -> GeoDecision {
        let code = country
            .and_then(|c| CountryCode::parse(c).ok())
            .filter(|c| !c.is_unknown_placeholder());
        match code {
            Some(code) if self.portal_blocked.contains(&code) => GeoDecision::Block {
                code: GeoBlockCode::GeoBlockedPortal,
                reason: "country is on the portal-wide block list",
            },
            Some(code) => match surface {
                Surface::Live if self.live_only_blocked.contains(&code) => GeoDecision::Block {
                    code: GeoBlockCode::GeoBlockedLive,
                    reason: "country is on the live-only block list",
                },
                Surface::Live | Surface::Portal => GeoDecision::Allow,
            },
            None => match surface {
                Surface::Live => GeoDecision::Block {
                    code: GeoBlockCode::GeoBlockedLive,
                    reason: "country unknown (missing, XX, T1 or malformed CF-IPCountry)",
                },
                Surface::Portal => GeoDecision::Allow,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_lists_match_the_owner_decision() {
        let p = GeoPolicy::default_policy();
        assert_eq!(p.portal_blocked().len(), 23);
        for code in [
            "UA", "HK", "MO", "CN", "RU", "IR", "KP", "CU", "SY", "VE", "EH",
        ] {
            assert!(
                p.portal_blocked()
                    .contains(&CountryCode::parse(code).unwrap()),
                "{code}"
            );
        }
        assert_eq!(
            p.live_only_blocked()
                .iter()
                .map(|c| c.as_str())
                .collect::<Vec<_>>(),
            vec!["KR"]
        );
    }

    #[test]
    fn list_parsing_refuses_junk_and_empty() {
        assert!(matches!(parse_country_list(""), Err(GeoConfigError::Empty)));
        assert!(matches!(
            parse_country_list(" , "),
            Err(GeoConfigError::Empty)
        ));
        assert!(parse_country_list("USA").is_err());
        assert!(parse_country_list("U1").is_err());
        assert!(
            parse_country_list("XX").is_err(),
            "XX is a placeholder, not a country"
        );
        let set = parse_country_list("kr, KR ua").unwrap();
        assert_eq!(
            set.iter().map(|c| c.as_str()).collect::<Vec<_>>(),
            vec!["KR", "UA"]
        );
    }
}
