// SPDX-License-Identifier: Apache-2.0
//! Geographic policy tests (owner decision 2026-10-27, ABUSE-11).

use consensus_api::live::geo::{
    parse_country_list, CountryCode, GeoBlockCode, GeoDecision, GeoPolicy, Surface,
    HTTP_STATUS_BLOCKED,
};

fn block_code(decision: &GeoDecision) -> Option<GeoBlockCode> {
    match decision {
        GeoDecision::Allow => None,
        GeoDecision::Block { code, .. } => Some(*code),
    }
}

#[test]
fn ukraine_is_blocked_on_the_whole_portal() {
    let p = GeoPolicy::default_policy();
    for surface in [Surface::Portal, Surface::Live] {
        let d = p.decide(Some("UA"), surface);
        assert_eq!(
            block_code(&d),
            Some(GeoBlockCode::GeoBlockedPortal),
            "{surface:?}"
        );
        assert_eq!(d.http_status(), Some(451));
        assert_eq!(HTTP_STATUS_BLOCKED, 451);
    }
}

#[test]
fn south_korea_is_blocked_for_live_mode_only() {
    let p = GeoPolicy::default_policy();
    assert_eq!(p.decide(Some("KR"), Surface::Portal), GeoDecision::Allow);
    let live = p.decide(Some("KR"), Surface::Live);
    assert_eq!(block_code(&live), Some(GeoBlockCode::GeoBlockedLive));
    assert_eq!(GeoBlockCode::GeoBlockedLive.as_str(), "geo_blocked_live");
    assert_eq!(
        GeoBlockCode::GeoBlockedPortal.as_str(),
        "geo_blocked_portal"
    );
}

#[test]
fn hong_kong_and_macao_are_blocked_like_the_other_restricted_territories() {
    let p = GeoPolicy::default_policy();
    for code in [
        "HK", "MO", "CN", "RU", "IR", "KP", "CU", "SY", "VE", "BY", "AF", "KH", "ER", "ET", "HT",
        "IQ", "LY", "MM", "NI", "PK", "SO", "EH",
    ] {
        for surface in [Surface::Portal, Surface::Live] {
            assert_eq!(
                block_code(&p.decide(Some(code), surface)),
                Some(GeoBlockCode::GeoBlockedPortal),
                "{code} {surface:?}"
            );
        }
    }
}

#[test]
fn unknown_country_is_refused_for_live_and_allowed_on_the_portal() {
    let p = GeoPolicy::default_policy();
    for unknown in [
        None,
        Some("XX"),
        Some("T1"),
        Some(""),
        Some("Z"),
        Some("USA"),
        Some("1A"),
    ] {
        assert_eq!(
            p.decide(unknown, Surface::Portal),
            GeoDecision::Allow,
            "{unknown:?}"
        );
        let live = p.decide(unknown, Surface::Live);
        assert_eq!(
            block_code(&live),
            Some(GeoBlockCode::GeoBlockedLive),
            "{unknown:?}"
        );
        assert_eq!(live.http_status(), Some(451));
    }
}

#[test]
fn allowed_countries_pass_both_surfaces_and_lowercase_input_is_normalized() {
    let p = GeoPolicy::default_policy();
    for code in ["US", "us", "DE", "br", "Mx", "JP", "FR"] {
        assert!(p.decide(Some(code), Surface::Portal).is_allowed(), "{code}");
        assert!(p.decide(Some(code), Surface::Live).is_allowed(), "{code}");
    }
    assert_eq!(
        block_code(&p.decide(Some("ua"), Surface::Portal)),
        Some(GeoBlockCode::GeoBlockedPortal)
    );
    assert_eq!(
        block_code(&p.decide(Some(" kr "), Surface::Live)),
        Some(GeoBlockCode::GeoBlockedLive)
    );
    assert_eq!(CountryCode::parse("ua").unwrap().as_str(), "UA");
}

#[test]
fn custom_lists_drive_the_policy() {
    let p = GeoPolicy::new(
        parse_country_list("FR").unwrap(),
        parse_country_list("de, it").unwrap(),
    );
    // Only the configured lists block: UA is not on this policy's lists.
    assert!(p.decide(Some("UA"), Surface::Live).is_allowed());
    assert!(p.decide(Some("UA"), Surface::Portal).is_allowed());
    assert_eq!(
        block_code(&p.decide(Some("FR"), Surface::Portal)),
        Some(GeoBlockCode::GeoBlockedPortal)
    );
    assert_eq!(
        block_code(&p.decide(Some("DE"), Surface::Live)),
        Some(GeoBlockCode::GeoBlockedLive)
    );
    assert!(p.decide(Some("DE"), Surface::Portal).is_allowed());
    assert_eq!(p.live_only_blocked().len(), 2);
}
