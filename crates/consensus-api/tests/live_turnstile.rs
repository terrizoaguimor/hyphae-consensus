// SPDX-License-Identifier: Apache-2.0
//! Turnstile verification tests (ABUSE-9): action, hostname, freshness,
//! outages, idempotent retries, the HTTP form, and redaction.

use chrono::{DateTime, TimeDelta, Utc};
use consensus_api::live::turnstile::{
    passing_answer, FakeSiteverify, HttpSiteverify, Siteverify, SiteverifyResponse,
    SiteverifyTransport, TurnstileError, TurnstileSecret, TurnstileVerifier, MAX_TOKEN_LEN,
};
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, TcpListener};
use std::sync::{Arc, Mutex};

const HOST: &str = "demo.example.org";
const LIVE: &str = "live-run";
const NOW: &str = "2026-10-27T12:00:00Z";

fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(NOW)
        .unwrap()
        .with_timezone(&Utc)
}

fn verifier(transport: Arc<dyn SiteverifyTransport>) -> TurnstileVerifier {
    TurnstileVerifier::new(transport, HOST, LIVE).with_clock(Arc::new(now))
}

fn fresh() -> SiteverifyResponse {
    passing_answer(HOST, LIVE, now() - TimeDelta::seconds(5))
}

fn ip() -> Option<IpAddr> {
    Some(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 9)))
}

#[test]
fn a_matching_fresh_token_passes_and_the_call_carries_ip_and_key() {
    let fake = Arc::new(FakeSiteverify::always(Ok(fresh())));
    let v = verifier(fake.clone());
    let verified = v.verify("tok-1", ip(), "key-1").unwrap();
    assert_eq!(verified.action, LIVE);
    assert_eq!(verified.hostname, HOST);
    assert_eq!(verified.challenge_ts, now() - TimeDelta::seconds(5));
    let calls = fake.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].token, "tok-1");
    assert_eq!(calls[0].remote_ip, ip());
    assert_eq!(calls[0].idempotency_key, "key-1");
}

#[test]
fn hostname_comparison_ignores_case() {
    let mut answer = fresh();
    answer.hostname = Some("Demo.Example.ORG".into());
    let v = verifier(Arc::new(FakeSiteverify::always(Ok(answer))));
    assert!(v.verify("tok", None, "k").is_ok());
}

#[test]
fn wrong_action_is_refused() {
    let mut answer = fresh();
    answer.action = Some("session".into());
    let v = verifier(Arc::new(FakeSiteverify::always(Ok(answer))));
    match v.verify("tok", ip(), "k") {
        Err(TurnstileError::WrongAction { expected, got }) => {
            assert_eq!(expected, LIVE);
            assert_eq!(got.as_deref(), Some("session"));
        }
        other => panic!("{other:?}"),
    }
    // ...and a missing action is not a match either.
    let mut answer = fresh();
    answer.action = None;
    let v = verifier(Arc::new(FakeSiteverify::always(Ok(answer))));
    assert!(matches!(
        v.verify("tok", ip(), "k"),
        Err(TurnstileError::WrongAction { .. })
    ));
}

#[test]
fn wrong_hostname_is_refused() {
    let mut answer = fresh();
    answer.hostname = Some("evil.example.net".into());
    let v = verifier(Arc::new(FakeSiteverify::always(Ok(answer))));
    match v.verify("tok", ip(), "k") {
        Err(TurnstileError::WrongHostname { expected, got }) => {
            assert_eq!(expected, HOST);
            assert_eq!(got.as_deref(), Some("evil.example.net"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn stale_or_far_future_challenge_ts_is_refused() {
    let stale = passing_answer(HOST, LIVE, now() - TimeDelta::seconds(301));
    let v = verifier(Arc::new(FakeSiteverify::always(Ok(stale))));
    match v.verify("tok", ip(), "k") {
        Err(TurnstileError::StaleChallenge { age_secs, max_secs }) => {
            assert_eq!((age_secs, max_secs), (301, 300));
        }
        other => panic!("{other:?}"),
    }
    let edge = passing_answer(HOST, LIVE, now() - TimeDelta::seconds(300));
    assert!(verifier(Arc::new(FakeSiteverify::always(Ok(edge))))
        .verify("tok", ip(), "k")
        .is_ok());
    let skewed = passing_answer(HOST, LIVE, now() + TimeDelta::seconds(30));
    assert!(verifier(Arc::new(FakeSiteverify::always(Ok(skewed))))
        .verify("tok", ip(), "k")
        .is_ok());
    let future = passing_answer(HOST, LIVE, now() + TimeDelta::seconds(120));
    assert!(matches!(
        verifier(Arc::new(FakeSiteverify::always(Ok(future)))).verify("tok", ip(), "k"),
        Err(TurnstileError::StaleChallenge { .. })
    ));
    let mut missing = fresh();
    missing.challenge_ts = None;
    assert!(matches!(
        verifier(Arc::new(FakeSiteverify::always(Ok(missing)))).verify("tok", ip(), "k"),
        Err(TurnstileError::BadChallengeTs(None))
    ));
    let mut garbage = fresh();
    garbage.challenge_ts = Some("yesterday".into());
    assert!(matches!(
        verifier(Arc::new(FakeSiteverify::always(Ok(garbage)))).verify("tok", ip(), "k"),
        Err(TurnstileError::BadChallengeTs(Some(_)))
    ));
}

#[test]
fn success_false_and_outages_fail_closed() {
    let failed = SiteverifyResponse {
        success: false,
        error_codes: vec!["invalid-input-response".into()],
        ..fresh()
    };
    match verifier(Arc::new(FakeSiteverify::always(Ok(failed)))).verify("tok", ip(), "k") {
        Err(TurnstileError::Failed { error_codes }) => {
            assert_eq!(error_codes, vec!["invalid-input-response"]);
        }
        other => panic!("{other:?}"),
    }
    // success=true with a passing action/hostname but an error list is
    // still only accepted on success; success=false never passes even
    // when everything else matches.
    let outage = Err(TurnstileError::Outage("connection reset".into()));
    let err = verifier(Arc::new(FakeSiteverify::always(outage)))
        .verify("tok", ip(), "k")
        .unwrap_err();
    assert!(err.is_outage());
    assert_eq!(err.public_code(), "turnstile_failed");
}

#[test]
fn empty_and_oversized_tokens_never_reach_cloudflare() {
    let fake = Arc::new(FakeSiteverify::always(Ok(fresh())));
    let v = verifier(fake.clone());
    assert!(matches!(
        v.verify("", ip(), "k"),
        Err(TurnstileError::EmptyToken)
    ));
    let long = "x".repeat(MAX_TOKEN_LEN + 1);
    assert!(matches!(
        v.verify(&long, ip(), "k"),
        Err(TurnstileError::TokenTooLong { .. })
    ));
    assert!(fake.calls().is_empty());
    assert!(v.verify(&"x".repeat(MAX_TOKEN_LEN), ip(), "k").is_ok());
}

#[test]
fn a_retry_with_the_same_idempotency_key_passes_and_a_new_key_does_not() {
    // Models Cloudflare: a token is single-use unless re-verified with the
    // same idempotency key.
    let fake = Arc::new(FakeSiteverify::new(|call, earlier| {
        let reused_elsewhere = earlier
            .iter()
            .any(|c| c.token == call.token && c.idempotency_key != call.idempotency_key);
        if reused_elsewhere {
            Ok(SiteverifyResponse {
                success: false,
                error_codes: vec!["timeout-or-duplicate".into()],
                ..SiteverifyResponse::default()
            })
        } else {
            Ok(passing_answer(HOST, LIVE, now() - TimeDelta::seconds(1)))
        }
    }));
    let v = verifier(fake.clone());
    assert!(v.verify("tok", ip(), "create-7").is_ok());
    assert!(
        v.verify("tok", ip(), "create-7").is_ok(),
        "same key: idempotent retry"
    );
    match v.verify("tok", ip(), "create-8") {
        Err(TurnstileError::Failed { error_codes }) => {
            assert_eq!(error_codes, vec!["timeout-or-duplicate"]);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(fake.calls().len(), 3);
}

#[test]
fn the_same_token_and_key_pass_twice_so_the_server_lookup_must_come_first() {
    // Cloudflare's documented behaviour, modelled by the fake: a token
    // already verified under key K verifies again under K. Nothing in this
    // module can tell the second call from the first, so the server's
    // owner-scoped idempotency lookup has to answer a repeated key (and
    // refuse a repeated key with a different body or preset) before
    // `verify` is ever called; otherwise one solved challenge could start
    // a second, different live run.
    let fake = Arc::new(FakeSiteverify::new(|call, earlier| {
        let spent_under_another_key = earlier
            .iter()
            .any(|c| c.token == call.token && c.idempotency_key != call.idempotency_key);
        if spent_under_another_key {
            Ok(SiteverifyResponse {
                success: false,
                error_codes: vec!["timeout-or-duplicate".into()],
                ..SiteverifyResponse::default()
            })
        } else {
            Ok(passing_answer(HOST, LIVE, now() - TimeDelta::seconds(1)))
        }
    }));
    let v = verifier(fake.clone());
    let first = v
        .verify("tok-solved-once", ip(), "owner-a:create-1")
        .unwrap();
    let second = v
        .verify("tok-solved-once", ip(), "owner-a:create-1")
        .unwrap();
    assert_eq!(first, second, "indistinguishable from a legitimate retry");
    assert_eq!(
        fake.calls().len(),
        2,
        "both reached Siteverify: this module alone cannot stop a replay under the same key"
    );
    // Under a new key the token is spent: the only replay Siteverify
    // itself refuses.
    assert!(matches!(
        v.verify("tok-solved-once", ip(), "owner-a:create-2"),
        Err(TurnstileError::Failed { .. })
    ));
}

#[test]
fn for_action_shares_the_transport_and_checks_its_own_action() {
    let fake = Arc::new(FakeSiteverify::new(|call, _| {
        let action = if call.token.starts_with("s-") {
            "session"
        } else {
            LIVE
        };
        Ok(passing_answer(HOST, action, now()))
    }));
    let live = verifier(fake.clone());
    let session = live.for_action("session");
    assert_eq!(session.expected_action(), "session");
    assert_eq!(session.expected_hostname(), HOST);
    assert!(session.verify("s-1", None, "k").is_ok());
    assert!(live.verify("l-1", None, "k").is_ok());
    assert!(
        matches!(
            live.verify("s-2", None, "k"),
            Err(TurnstileError::WrongAction { .. })
        ),
        "a session token cannot start a live run"
    );
    assert!(matches!(
        session.verify("l-2", None, "k"),
        Err(TurnstileError::WrongAction { .. })
    ));
    assert_eq!(fake.calls().len(), 4);
}

/// Serves one canned response to every connection and records bodies.
struct FakeServer {
    endpoint: String,
    bodies: Arc<Mutex<Vec<String>>>,
}

fn serve(status: u16, body: &'static str) -> FakeServer {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let seen = bodies.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let mut data = Vec::new();
            let mut buf = [0u8; 4096];
            let mut content_length = None;
            let mut header_end = None;
            loop {
                let n = match s.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                data.extend_from_slice(&buf[..n]);
                if header_end.is_none() {
                    if let Some(idx) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                        header_end = Some(idx + 4);
                        let head = String::from_utf8_lossy(&data[..idx]).to_lowercase();
                        content_length = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .and_then(|v| v.trim().parse::<usize>().ok());
                    }
                }
                if let (Some(end), Some(len)) = (header_end, content_length) {
                    if data.len() >= end + len {
                        break;
                    }
                }
            }
            let end = header_end.unwrap_or(data.len());
            seen.lock()
                .unwrap()
                .push(String::from_utf8_lossy(&data[end..]).to_string());
            let resp = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = s.write_all(resp.as_bytes());
        }
    });
    FakeServer {
        endpoint: format!("http://{addr}/turnstile/v0/siteverify"),
        bodies,
    }
}

#[test]
fn http_transport_posts_the_documented_form_and_parses_the_answer() {
    let server = serve(
        200,
        r#"{"success":true,"challenge_ts":"2026-10-27T11:59:55.000Z","hostname":"demo.example.org","error-codes":[],"action":"live-run","cdata":"c1"}"#,
    );
    let secret = TurnstileSecret::new("0x4AAAAAAAsecret-value-9").unwrap();
    let http = Arc::new(HttpSiteverify::with_endpoint(
        secret,
        server.endpoint.clone(),
    ));
    let v = verifier(http.clone());
    let verified = v.verify("tok-abc", ip(), "idem-42").unwrap();
    assert_eq!(verified.cdata.as_deref(), Some("c1"));
    let bodies = server.bodies.lock().unwrap();
    assert_eq!(bodies.len(), 1);
    let pairs: Vec<(&str, &str)> = bodies[0]
        .split('&')
        .map(|kv| kv.split_once('=').unwrap())
        .collect();
    assert!(pairs.contains(&("secret", "0x4AAAAAAAsecret-value-9")));
    assert!(pairs.contains(&("response", "tok-abc")));
    assert!(pairs.contains(&("idempotency_key", "idem-42")));
    assert!(pairs.contains(&("remoteip", "203.0.113.9")));
    assert_eq!(pairs.len(), 4);
    // Nothing printable about the client reveals the secret.
    let debug = format!("{http:?} {v:?}");
    assert!(!debug.contains("secret-value"), "{debug}");
    assert!(debug.contains("<redacted>"));
}

#[test]
fn http_transport_treats_non_200_and_unparseable_bodies_as_refusals() {
    let secret = TurnstileSecret::new("0x4AAAAAAAsecret").unwrap();
    let five_hundred = serve(500, "upstream error");
    let v = verifier(Arc::new(HttpSiteverify::with_endpoint(
        secret.clone(),
        five_hundred.endpoint.clone(),
    )));
    match v.verify("tok", None, "k") {
        Err(TurnstileError::Outage(msg)) => assert!(msg.contains("500"), "{msg}"),
        other => panic!("{other:?}"),
    }
    let garbage = serve(200, "<html>not json</html>");
    let v = verifier(Arc::new(HttpSiteverify::with_endpoint(
        secret.clone(),
        garbage.endpoint.clone(),
    )));
    assert!(matches!(
        v.verify("tok", None, "k"),
        Err(TurnstileError::Malformed(_))
    ));
    // A closed port: the connection is refused, so is the visitor.
    let closed = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = closed.local_addr().unwrap();
    drop(closed);
    let v = verifier(Arc::new(HttpSiteverify::with_endpoint(
        secret,
        format!("http://{addr}/siteverify"),
    )));
    let err = v.verify("tok", None, "k").unwrap_err();
    assert!(err.is_outage(), "{err:?}");
}

#[test]
fn test_secrets_are_recognized_and_real_ones_are_not() {
    for test in [
        "1x0000000000000000000000000000000AA",
        "2x0000000000000000000000000000000AA",
        "3x0000000000000000000000000000000AA",
    ] {
        assert!(
            TurnstileSecret::new(test).unwrap().is_test_secret(),
            "{test}"
        );
    }
    assert!(!TurnstileSecret::new("0x4AAAAAAAreal")
        .unwrap()
        .is_test_secret());
    let dir = std::env::temp_dir().join(format!(
        "hyphae-consensus-live-turnstile-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("secret");
    std::fs::write(&path, "0x4AAAAAAAfromfile\n").unwrap();
    let secret = TurnstileSecret::from_file(&path).unwrap();
    assert!(!secret.is_test_secret());
    assert_eq!(format!("{secret:?}"), "TurnstileSecret(<redacted>)");
    let _ = std::fs::remove_dir_all(&dir);
}
