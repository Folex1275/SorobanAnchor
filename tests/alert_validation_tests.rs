//! Integration coverage for alert dedup/suppression validation and routing
//! config hardening (#1053, #1054, #1055, #1056).
//!
//! These scenarios are written against the public API so they run even if the
//! crate's in-module unit-test binary is unavailable.

use anchorkit::alert_dedup::{
    AlertDeduplicator, AlertSuppressor, DedupConfig,
};
use anchorkit::alert_routing::{
    AlertRoute, AlertRouterConfig, AlertSeverity,
};

// ---------------------------------------------------------------------------
// #1055 — reject expired alert suppressions
// ---------------------------------------------------------------------------

#[test]
fn zero_duration_suppression_is_rejected() {
    let s = AlertSuppressor::new();
    s.suppress("anchor:example.com", 1_000, 1_000, "zero duration");
    assert!(!s.is_suppressed("anchor:example.com", 1_000));
    assert_eq!(s.active_count(1_000), 0);
}

#[test]
fn already_expired_suppression_is_rejected() {
    let s = AlertSuppressor::new();
    s.suppress("anchor:example.com", 1_000, 900, "already past");
    assert!(!s.is_suppressed("anchor:example.com", 1_000));
    assert_eq!(s.active_count(1_000), 0);
}

#[test]
fn expired_suppression_never_appears_in_entries() {
    let s = AlertSuppressor::new();
    s.suppress("anchor:example.com", 1_000, 500, "long past");
    assert!(
        s.entries().is_empty(),
        "rejected suppression must not occupy suppression state"
    );
}

#[test]
fn future_dated_suppression_still_works() {
    let s = AlertSuppressor::new();
    s.suppress("anchor:example.com", 1_000, 2_000, "maintenance");
    assert!(s.is_suppressed("anchor:example.com", 1_500));
    assert_eq!(s.active_count(1_500), 1);
}

// ---------------------------------------------------------------------------
// #1288 — alert deduplication window boundaries (fixed timestamps)
// ---------------------------------------------------------------------------

#[test]
fn repeat_immediately_inside_window_is_suppressed() {
    let d = AlertDeduplicator::new(DedupConfig {
        window_seconds: 300,
        max_keys: 100,
    });
    assert!(d.should_fire("anchor:example.com", 1_000), "first alert fires");
    assert!(
        !d.should_fire("anchor:example.com", 1_001),
        "repeat one second inside the window must be suppressed"
    );
    assert!(
        !d.should_fire("anchor:example.com", 1_299),
        "repeat just before the boundary must be suppressed"
    );
}

#[test]
fn repeat_exactly_at_window_boundary_is_accepted() {
    let d = AlertDeduplicator::new(DedupConfig {
        window_seconds: 300,
        max_keys: 100,
    });
    assert!(d.should_fire("anchor:example.com", 1_000));
    assert!(
        d.should_fire("anchor:example.com", 1_300),
        "now - last == window_seconds is outside the window"
    );
}

#[test]
fn repeat_just_outside_window_is_accepted() {
    let d = AlertDeduplicator::new(DedupConfig {
        window_seconds: 300,
        max_keys: 100,
    });
    assert!(d.should_fire("anchor:example.com", 1_000));
    assert!(
        d.should_fire("anchor:example.com", 1_301),
        "repeat past the window must be accepted"
    );
}

#[test]
fn distinct_keys_are_deduplicated_independently() {
    let d = AlertDeduplicator::new(DedupConfig {
        window_seconds: 300,
        max_keys: 100,
    });
    assert!(d.should_fire("anchor:a", 1_000));
    assert!(
        d.should_fire("anchor:b", 1_000),
        "a different key is unaffected by another key's window"
    );
    assert!(!d.should_fire("anchor:a", 1_100));
    assert!(
        d.should_fire("anchor:b", 1_300),
        "each key expires on its own timestamp"
    );
    assert!(
        !d.should_fire("anchor:a", 1_299),
        "key a is still inside its own window"
    );
}

// ---------------------------------------------------------------------------
// #1054 — escape alert fingerprints
// ---------------------------------------------------------------------------

#[test]
fn suppression_log_escapes_hostile_fingerprint() {
    let d = AlertDeduplicator::new(DedupConfig {
        window_seconds: 300,
        max_keys: 100,
    });
    let tricky = "anchor\"key\\with\nnewline";
    assert!(d.should_fire(tricky, 1_000), "first occurrence fires");
    assert!(!d.should_fire(tricky, 1_100), "repeat suppressed");

    let log = d.drain_suppression_log();
    assert_eq!(log.len(), 1);
    // Expected JSON text: {"alert_key":"anchor\"key\\with\nnewline"}
    let expected = "{\"alert_key\":\"anchor\\\"key\\\\with\\nnewline\"}";
    assert_eq!(log[0], expected, "fingerprint must be JSON-escaped verbatim");
    assert!(
        !log[0].contains('\n'),
        "raw newline would break the JSON line"
    );
}

#[test]
fn simple_fingerprints_produce_identical_semantic_output() {
    let d = AlertDeduplicator::new(DedupConfig {
        window_seconds: 300,
        max_keys: 100,
    });
    assert!(d.should_fire("anchor:example.com:critical", 1_000));
    assert!(!d.should_fire("anchor:example.com:critical", 1_100));
    let log = d.drain_suppression_log();
    assert_eq!(
        log[0],
        "{\"alert_key\":\"anchor:example.com:critical\"}",
        "ordinary fingerprints must serialize exactly as before"
    );
}

// ---------------------------------------------------------------------------
// #1053 — bound suppression diagnostics
// ---------------------------------------------------------------------------

#[test]
fn repeated_suppression_is_bounded_by_max_keys() {
    let d = AlertDeduplicator::new(DedupConfig {
        window_seconds: 300,
        max_keys: 2,
    });
    assert!(d.should_fire("k1", 1_000));
    for i in 1..=5u64 {
        assert!(!d.should_fire("k1", 1_000 + i));
    }
    let log = d.drain_suppression_log();
    assert_eq!(
        log.len(),
        2,
        "suppression diagnostics must stay capped at max_keys"
    );
    assert_eq!(d.tracked_count(), 1, "alert-key map stays bounded too");
}

#[test]
fn newest_suppression_diagnostic_is_retained() {
    let d = AlertDeduplicator::new(DedupConfig {
        window_seconds: 300,
        max_keys: 3,
    });
    assert!(d.should_fire("anchor:a", 1_000));
    for i in 1..=5u64 {
        assert!(!d.should_fire("anchor:a", 1_000 + i));
    }
    let log = d.drain_suppression_log();
    assert_eq!(log.len(), 3);
    // Every diagnostic is well-formed JSON for the same alert key.
    for entry in &log {
        assert!(
            entry.starts_with("{\"alert_key\":") && entry.ends_with('}'),
            "diagnostic must be a JSON object: {entry}"
        );
        assert!(entry.contains("anchor:a"));
    }
}

#[test]
fn ordinary_single_suppression_is_unchanged() {
    let d = AlertDeduplicator::new(DedupConfig {
        window_seconds: 300,
        max_keys: 2,
    });
    assert!(d.should_fire("k1", 1_000));
    assert!(!d.should_fire("k1", 1_100));
    assert_eq!(d.drain_suppression_log().len(), 1);
}

// ---------------------------------------------------------------------------
// #1056 — reject unknown alert severity
// ---------------------------------------------------------------------------

fn monitoring_with_severities(severities: &[&str]) -> anchorkit::config::MonitoringConfig {
    let alerts = severities
        .iter()
        .map(|sev| anchorkit::config::AlertConfig {
            condition: "payments".into(),
            severity: (*sev).into(),
            recipients: vec!["https://hooks.example.com/a".into()],
            extra: std::collections::BTreeMap::new(),
        })
        .collect::<Vec<_>>();
    anchorkit::config::MonitoringConfig {
        enable_metrics: None,
        log_all_operations: None,
        alert_on_failed_attestations: None,
        alert_on_replay_attempts: None,
        metrics_namespace: None,
        alerts: Some(alerts),
    }
}

#[test]
fn from_monitoring_config_accepts_known_severities() {
    let config = monitoring_with_severities(&["critical", "warning", "info", "error"]);
    let router = AlertRouterConfig::from_monitoring_config(Some(&config))
        .expect("known severity names parse normally");
    assert_eq!(router.rules.len(), 4);
    assert_eq!(router.rules[0].severity, AlertSeverity::Critical);
    assert_eq!(router.rules[3].severity, AlertSeverity::Error);
    assert!(router.rules[0].routes[0].channel == "webhook");
}

#[test]
fn from_monitoring_config_rejects_unknown_severity() {
    let config = monitoring_with_severities(&["urgent"]);
    let err = AlertRouterConfig::from_monitoring_config(Some(&config))
        .expect_err("unknown severity must fail configuration");
    assert_eq!(err.code, anchorkit::errors::ErrorCode::ValidationError);
    let context = err.context.as_deref().unwrap_or("");
    assert!(
        context.contains("severity") && context.contains("urgent"),
        "error must identify the offending field and value: {context}"
    );
}

#[test]
fn from_monitoring_config_rejects_blank_severity() {
    let config = monitoring_with_severities(&[""]);
    assert!(
        AlertRouterConfig::from_monitoring_config(Some(&config)).is_err(),
        "blank severity must fail rather than default to Warning"
    );
}

#[test]
fn from_monitoring_config_accepts_none() {
    assert!(AlertRouterConfig::from_monitoring_config(None).is_ok());
    assert!(AlertRouterConfig::from_monitoring_config(
        Some(&anchorkit::config::MonitoringConfig {
            enable_metrics: None,
            log_all_operations: None,
            alert_on_failed_attestations: None,
            alert_on_replay_attempts: None,
            metrics_namespace: None,
            alerts: None,
        })
    )
    .is_ok());
}

#[test]
fn from_monitoring_config_fills_routes_from_recipients() {
    let config = anchorkit::config::MonitoringConfig {
        enable_metrics: None,
        log_all_operations: None,
        alert_on_failed_attestations: None,
        alert_on_replay_attempts: None,
        metrics_namespace: None,
        alerts: Some(vec![anchorkit::config::AlertConfig {
            condition: "payments".into(),
            severity: "critical".into(),
            recipients: vec![
                "https://hooks.example.com/1".into(),
                "https://hooks.example.com/2".into(),
            ],
            extra: std::collections::BTreeMap::new(),
        }]),
    };
    let router = AlertRouterConfig::from_monitoring_config(Some(&config)).unwrap();
    assert_eq!(router.rules.len(), 1);
    assert_eq!(router.rules[0].routes.len(), 2);
    let _ = AlertRoute::webhook("https://hooks.example.com/ignored");
}