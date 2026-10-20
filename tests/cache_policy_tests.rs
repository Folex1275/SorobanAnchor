//! Tests for the cache governance policy model.
//!
//! Covers:
//! - TTL clamping (min / max bounds)
//! - Refresh threshold detection
//! - Expiry detection
//! - Forced-invalidation guard
//! - Policy set persistence (get/set round-trip)
//! - Integration with contract cache_metadata / force_refresh_metadata

#![cfg(test)]

use soroban_sdk::testutils::{Address as _, Ledger, LedgerInfo};
use soroban_sdk::{Address, Env};
use anchorkit::cache_governance::{
    self, CacheEntryType, CachePolicy, CachePolicySet,
    enforce_write_policy, enforce_read_policy, enforce_invalidation_policy,
};
use anchorkit::contract::{AnchorKitContract, AnchorKitContractClient, AnchorMetadata};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn set_ledger(env: &Env, seq: u32, ts: u64) {
    env.ledger().set(LedgerInfo {
        timestamp: ts,
        protocol_version: 21,
        sequence_number: seq,
        network_id: Default::default(),
        base_reserve: 0,
        min_persistent_entry_ttl: 4096,
        min_temp_entry_ttl: 16,
        max_entry_ttl: 6_312_000,
    });
}

fn sample_metadata(env: &Env, anchor: &Address) -> AnchorMetadata {
    AnchorMetadata {
        anchor: anchor.clone(),
        reputation_score: 80,
        liquidity_score: 90,
        uptime_percentage: 99,
        total_volume: 1_000_000,
        average_settlement_time: 120,
        is_active: true,
    }
}

// ---------------------------------------------------------------------------
// CachePolicy unit tests  (no Soroban env needed)
// ---------------------------------------------------------------------------

#[test]
fn test_default_metadata_policy_valid() {
    let p = CachePolicy::default_metadata();
    assert!(p.validate().is_ok());
    assert_eq!(p.min_ttl_seconds, 60);
    assert_eq!(p.max_ttl_seconds, 86_400);
    assert_eq!(p.refresh_threshold_pct, 80);
    assert!(p.allow_forced_invalidation);
}

#[test]
fn test_default_capabilities_policy_valid() {
    let p = CachePolicy::default_capabilities();
    assert!(p.validate().is_ok());
    assert_eq!(p.min_ttl_seconds, 300);
    assert_eq!(p.max_ttl_seconds, 604_800);
    assert_eq!(p.refresh_threshold_pct, 75);
    assert!(p.allow_forced_invalidation);
}

#[test]
fn test_default_other_policy_valid() {
    let p = CachePolicy::default_other();
    assert!(p.validate().is_ok());
    assert!(!p.allow_forced_invalidation);
}

#[test]
fn test_policy_validate_rejects_zero_min() {
    let p = CachePolicy {
        min_ttl_seconds: 0,
        max_ttl_seconds: 3600,
        refresh_threshold_pct: 80,
        allow_forced_invalidation: true,
    };
    assert!(p.validate().is_err());
}

#[test]
fn test_policy_validate_rejects_min_ge_max() {
    let p = CachePolicy {
        min_ttl_seconds: 3600,
        max_ttl_seconds: 3600,
        refresh_threshold_pct: 80,
        allow_forced_invalidation: true,
    };
    assert!(p.validate().is_err());

    let p2 = CachePolicy { min_ttl_seconds: 7200, ..p };
    assert!(p2.validate().is_err());
}

#[test]
fn test_policy_validate_rejects_invalid_threshold() {
    let base = CachePolicy {
        min_ttl_seconds: 60,
        max_ttl_seconds: 3600,
        refresh_threshold_pct: 0,
        allow_forced_invalidation: true,
    };
    assert!(base.validate().is_err(), "threshold 0 should be rejected");

    let p100 = CachePolicy { refresh_threshold_pct: 100, ..base };
    assert!(p100.validate().is_err(), "threshold 100 should be rejected");

    let p99 = CachePolicy { refresh_threshold_pct: 99, min_ttl_seconds: 60, max_ttl_seconds: 3600, allow_forced_invalidation: true };
    assert!(p99.validate().is_ok(), "threshold 99 should be accepted");
}

// ---------------------------------------------------------------------------
// TTL clamping (pure logic, no Soroban env)
// ---------------------------------------------------------------------------

#[test]
fn test_clamp_ttl_within_band_unchanged() {
    let p = CachePolicy::default_metadata(); // [60, 86400]
    assert_eq!(p.clamp_ttl(3_600), 3_600);
    assert_eq!(p.clamp_ttl(60), 60);
    assert_eq!(p.clamp_ttl(86_400), 86_400);
}

#[test]
fn test_clamp_ttl_below_min_clamped_up() {
    let p = CachePolicy::default_metadata(); // min = 60
    assert_eq!(p.clamp_ttl(1), 60);
    assert_eq!(p.clamp_ttl(59), 60);
}

#[test]
fn test_clamp_ttl_above_max_clamped_down() {
    let p = CachePolicy::default_metadata(); // max = 86400
    assert_eq!(p.clamp_ttl(100_000), 86_400);
    assert_eq!(p.clamp_ttl(u64::MAX), 86_400);
}

#[test]
fn test_clamp_ttl_zero_returns_midpoint() {
    let p = CachePolicy {
        min_ttl_seconds: 100,
        max_ttl_seconds: 300,
        refresh_threshold_pct: 80,
        allow_forced_invalidation: true,
    };
    // midpoint = 100 + (300-100)/2 = 200
    assert_eq!(p.clamp_ttl(0), 200);
}

// ---------------------------------------------------------------------------
// Refresh threshold (pure logic)
// ---------------------------------------------------------------------------

#[test]
fn test_needs_refresh_at_threshold() {
    let p = CachePolicy {
        min_ttl_seconds: 60,
        max_ttl_seconds: 3600,
        refresh_threshold_pct: 80,
        allow_forced_invalidation: true,
    };
    // 80% of 3600 = 2880
    assert!(!p.needs_refresh(2879, 3600), "age 2879 is before threshold");
    assert!(p.needs_refresh(2880, 3600), "age 2880 is at threshold");
    assert!(p.needs_refresh(3599, 3600), "age 3599 is past threshold but not expired");
}

#[test]
fn test_needs_refresh_false_when_fresh() {
    let p = CachePolicy::default_metadata();
    assert!(!p.needs_refresh(100, 3600));
}

// ---------------------------------------------------------------------------
// Expiry detection (pure logic)
// ---------------------------------------------------------------------------

#[test]
fn test_is_expired_at_ttl_boundary() {
    let p = CachePolicy::default_metadata();
    assert!(!p.is_expired(3599, 3600), "age 3599 not expired");
    assert!(p.is_expired(3600, 3600), "age 3600 is exactly expired");
    assert!(p.is_expired(3601, 3600), "age 3601 is past expired");
}

// ---------------------------------------------------------------------------
// TTL boundary semantics (fixed clock via set_ledger, no sleeps)
// ---------------------------------------------------------------------------

/// At the exact TTL boundary (age == ttl) the entry is still considered fresh
/// by the policy: it is not expired, though it is flagged for refresh.
#[test]
fn test_ttl_boundary_equality_is_fresh() {
    let env = make_env();
    let cid = env.register_contract(None, AnchorKitContract);
    set_ledger(&env, 1, 1000);

    env.as_contract(&cid, || {
        // age == ttl exactly.
        let (refresh, expired) = enforce_read_policy(&env, CacheEntryType::Metadata, 3600, 3600);
        assert!(refresh, "entry at TTL boundary should be flagged for refresh");
        assert!(!expired, "entry at exact TTL equality must not be expired");
    });
}

/// One second beyond the TTL (age == ttl + 1) the entry is expired and must
/// not be served as fresh.
#[test]
fn test_ttl_boundary_just_beyond_is_expired() {
    let env = make_env();
    let cid = env.register_contract(None, AnchorKitContract);
    set_ledger(&env, 1, 1000);

    env.as_contract(&cid, || {
        // age == ttl + 1.
        let (refresh, expired) = enforce_read_policy(&env, CacheEntryType::Metadata, 3601, 3600);
        assert!(refresh, "expired entry should also be flagged for refresh");
        assert!(expired, "entry one second past TTL must be expired");
    });
}

// ---------------------------------------------------------------------------
// enforce_write_policy / enforce_read_policy  (require Soroban env)
// ---------------------------------------------------------------------------

#[test]
fn test_enforce_write_policy_clamps_to_bounds() {
    let env = make_env();
    let cid = env.register_contract(None, AnchorKitContract);
    set_ledger(&env, 1, 1000);

    env.as_contract(&cid, || {
        // Metadata policy default: min=60, max=86400
        let (ttl, _) = enforce_write_policy(&env, CacheEntryType::Metadata, 10, 0);
        assert_eq!(ttl, 60, "TTL below min should be clamped up to 60");

        let (ttl2, _) = enforce_write_policy(&env, CacheEntryType::Metadata, 999_999, 0);
        assert_eq!(ttl2, 86_400, "TTL above max should be clamped down to 86400");

        let (ttl3, _) = enforce_write_policy(&env, CacheEntryType::Metadata, 3_600, 0);
        assert_eq!(ttl3, 3_600, "TTL within band should be unchanged");
    });
}

#[test]
fn test_enforce_read_policy_fresh_entry() {
    let env = make_env();
    let cid = env.register_contract(None, AnchorKitContract);
    set_ledger(&env, 1, 1000);

    env.as_contract(&cid, || {
        // age=100, ttl=3600 → 80% threshold=2880; age < 2880 → not stale, not expired
        let (refresh, expired) = enforce_read_policy(&env, CacheEntryType::Metadata, 100, 3600);
        assert!(!refresh);
        assert!(!expired);
    });
}

#[test]
fn test_enforce_read_policy_stale_entry() {
    let env = make_env();
    let cid = env.register_contract(None, AnchorKitContract);
    set_ledger(&env, 1, 1000);

    env.as_contract(&cid, || {
        // age=3000, ttl=3600 → 80%=2880; 3000 >= 2880 but < 3600 → refresh, not expired
        let (refresh, expired) = enforce_read_policy(&env, CacheEntryType::Metadata, 3000, 3600);
        assert!(refresh, "should need refresh");
        assert!(!expired, "should not be expired");
    });
}

#[test]
fn test_enforce_read_policy_expired_entry() {
    let env = make_env();
    let cid = env.register_contract(None, AnchorKitContract);
    set_ledger(&env, 1, 1000);

    env.as_contract(&cid, || {
        // age >= ttl → expired
        let (refresh, expired) = enforce_read_policy(&env, CacheEntryType::Metadata, 3600, 3600);
        assert!(refresh);
        assert!(expired);
    });
}

/// Zero TTL is rejected outright so it can never be classified as both expired
/// and refresh-needed for the policy's entry type.
#[test]
#[should_panic]
fn test_enforce_read_policy_rejects_zero_ttl() {
    let env = make_env();
    let cid = env.register_contract(None, AnchorKitContract);
    set_ledger(&env, 1, 1000);

    env.as_contract(&cid, || {
        let _ = enforce_read_policy(&env, CacheEntryType::Metadata, 0, 0);
    });
}

// ---------------------------------------------------------------------------
// enforce_invalidation_policy
// ---------------------------------------------------------------------------

#[test]
fn test_invalidation_allowed_by_default_for_metadata() {
    let env = make_env();
    let cid = env.register_contract(None, AnchorKitContract);
    set_ledger(&env, 1, 1000);

    env.as_contract(&cid, || {
        let result = enforce_invalidation_policy(&env, CacheEntryType::Metadata);
        assert!(result.is_ok(), "default metadata policy allows forced invalidation");
    });
}

#[test]
fn test_invalidation_blocked_when_policy_disables_it() {
    let env = make_env();
    let cid = env.register_contract(None, AnchorKitContract);
    set_ledger(&env, 1, 1000);

    env.as_contract(&cid, || {
        let mut ps = cache_governance::get_policy_set(&env);
        ps.metadata.allow_forced_invalidation = false;
        cache_governance::set_policy_set(&env, ps).unwrap();

        let result = enforce_invalidation_policy(&env, CacheEntryType::Metadata);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.context.as_deref().unwrap_or("").contains("forced invalidation"),
            "error context should mention forced invalidation"
        );
    });
}

// ---------------------------------------------------------------------------
// Policy set persistence round-trip
// ---------------------------------------------------------------------------

#[test]
fn test_policy_set_default_round_trip() {
    let env = make_env();
    let cid = env.register_contract(None, AnchorKitContract);
    set_ledger(&env, 1, 1000);

    env.as_contract(&cid, || {
        let set = cache_governance::get_policy_set(&env);
        assert_eq!(set, CachePolicySet::default_set());
    });
}

#[test]
fn test_policy_set_custom_persisted_and_retrieved() {
    let env = make_env();
    let cid = env.register_contract(None, AnchorKitContract);
    set_ledger(&env, 1, 1000);

    env.as_contract(&cid, || {
        let custom = CachePolicySet {
            metadata: CachePolicy {
                min_ttl_seconds: 120,
                max_ttl_seconds: 7_200,
                refresh_threshold_pct: 70,
                allow_forced_invalidation: true,
            },
            capabilities: CachePolicy::default_capabilities(),
            other: CachePolicy::default_other(),
        };
        cache_governance::set_policy_set(&env, custom).unwrap();

        let retrieved = cache_governance::get_policy_set(&env);
        assert_eq!(retrieved.metadata.min_ttl_seconds, 120);
        assert_eq!(retrieved.metadata.max_ttl_seconds, 7_200);
        assert_eq!(retrieved.metadata.refresh_threshold_pct, 70);
    });
}

#[test]
fn test_policy_set_invalid_rejected() {
    let env = make_env();
    let cid = env.register_contract(None, AnchorKitContract);
    set_ledger(&env, 1, 1000);

    env.as_contract(&cid, || {
        let bad = CachePolicySet {
            metadata: CachePolicy {
                min_ttl_seconds: 0, // invalid: must be > 0
                max_ttl_seconds: 3600,
                refresh_threshold_pct: 80,
                allow_forced_invalidation: true,
            },
            capabilities: CachePolicy::default_capabilities(),
            other: CachePolicy::default_other(),
        };
        let result = cache_governance::set_policy_set(&env, bad);
        assert!(result.is_err(), "invalid policy should be rejected");
    });
}

// ---------------------------------------------------------------------------
// Contract integration: cache_metadata respects policy TTL clamping
// ---------------------------------------------------------------------------

#[test]
fn test_contract_cache_metadata_clamps_ttl_to_policy_min() {
    let env = make_env();
    let cid = env.register_contract(None, AnchorKitContract);
    set_ledger(&env, 1, 1000);

    let client = AnchorKitContractClient::new(&env, &cid);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let anchor = Address::generate(&env);
    let meta = sample_metadata(&env, &anchor);

    // Request TTL=1 (below policy min of 60 s) — policy clamps it up to 60 s.
    client.cache_metadata(&anchor, &meta, &1u64);

    // At t=1059, age=59 s — still within the clamped 60 s TTL.
    set_ledger(&env, 2, 1059);
    let retrieved = client.get_cached_metadata(&anchor);
    assert_eq!(retrieved.anchor, anchor, "entry should still be valid at age 59 s");
}

/// Contract-level TTL boundary: at exact TTL equality the cached entry is
/// still returned (fresh), driven purely by the injected ledger timestamp.
#[test]
fn test_contract_cache_metadata_valid_at_exact_ttl() {
    let env = make_env();
    let cid = env.register_contract(None, AnchorKitContract);
    set_ledger(&env, 1, 1000);

    let client = AnchorKitContractClient::new(&env, &cid);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let anchor = Address::generate(&env);
    let meta = sample_metadata(&env, &anchor);

    // TTL=3600 is within the default metadata band [60, 86400].
    client.cache_metadata(&anchor, &meta, &3600u64);

    // At t=4600, age == 3600 exactly — boundary equality is still fresh.
    set_ledger(&env, 2, 4600);
    let retrieved = client.get_cached_metadata(&anchor);
    assert_eq!(retrieved.anchor, anchor, "entry at exact TTL equality should still be valid");
}

/// Contract-level TTL boundary: one second past TTL the entry is expired and
/// must not be returned as fresh.
#[test]
fn test_contract_cache_metadata_expired_just_beyond_ttl() {
    let env = make_env();
    let cid = env.register_contract(None, AnchorKitContract);
    set_ledger(&env, 1, 1000);

    let client = AnchorKitContractClient::new(&env, &cid);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let anchor = Address::generate(&env);
    let meta = sample_metadata(&env, &anchor);

    client.cache_metadata(&anchor, &meta, &3600u64);

    // At t=4601, age == 3601 (ttl + 1) — entry is expired.
    set_ledger(&env, 2, 4601);
    let result = client.try_get_cached_metadata(&anchor);
    assert!(result.is_err(), "entry one second past TTL must not be served as fresh");
}

#[test]
fn test_contract_cache_metadata_clamps_ttl_to_policy_max() {
    let env = make_env();
    let cid = env.register_contract(None, AnchorKitContract);
    set_ledger(&env, 1, 1000);

    let client = AnchorKitContractClient::new(&env, &cid);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Override policy to a tight max of 200 s so we can verify clamping.
    env.as_contract(&cid, || {
        let mut ps = cache_governance::get_policy_set(&env);
        ps.metadata.min_ttl_seconds = 10;
        ps.metadata.max_ttl_seconds = 200;
        cache_governance::set_policy_set(&env, ps).unwrap();
    });

    let anchor = Address::generate(&env);
    let meta = sample_metadata(&env, &anchor);

    // Request TTL=999_999 (way above max=200) — policy clamps it down to 200 s.
    client.cache_metadata(&anchor, &meta, &999_999u64);

    // At t=1201, age=201 s — past the clamped max of 200 s.
    set_ledger(&env, 2, 1201);
    let result = client.try_get_cached_metadata(&anchor);
    assert!(result.is_err(), "entry should be expired after 201 s with clamped max=200 s TTL");
}

// ---------------------------------------------------------------------------
// Contract integration: force_refresh_metadata blocked when policy disallows
// ---------------------------------------------------------------------------

#[test]
fn test_force_refresh_blocked_when_policy_disallows() {
    let env = make_env();
    let cid = env.register_contract(None, AnchorKitContract);
    set_ledger(&env, 1, 1000);

    let client = AnchorKitContractClient::new(&env, &cid);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Disable forced invalidation for metadata entries.
    env.as_contract(&cid, || {
        let mut ps = cache_governance::get_policy_set(&env);
        ps.metadata.allow_forced_invalidation = false;
        cache_governance::set_policy_set(&env, ps).unwrap();
    });

    let anchor = Address::generate(&env);
    let meta = sample_metadata(&env, &anchor);
    // Normal write is always allowed.
    client.cache_metadata(&anchor, &meta, &3600u64);

    // force_refresh should now be blocked by policy.
    let result = client.try_force_refresh_metadata(&anchor, &meta, &3600u64, &300u64);
    assert!(
        result.is_err(),
        "force_refresh_metadata must fail when policy disables forced invalidation"
    );
}

// ---------------------------------------------------------------------------
// Contract integration: set_cache_policy_set / get_cache_policy_set
// ---------------------------------------------------------------------------

#[test]
fn test_contract_get_set_cache_policy_set_round_trip() {
    let env = make_env();
    let cid = env.register_contract(None, AnchorKitContract);
    set_ledger(&env, 1, 1000);

    let client = AnchorKitContractClient::new(&env, &cid);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Default should come back as the built-in defaults.
    let default_ps = client.get_cache_policy_set();
    assert_eq!(default_ps.metadata.min_ttl_seconds, 60);
    assert_eq!(default_ps.metadata.max_ttl_seconds, 86_400);

    // Set a custom policy via the contract method.
    let custom = CachePolicySet {
        metadata: CachePolicy {
            min_ttl_seconds: 180,
            max_ttl_seconds: 10_800,
            refresh_threshold_pct: 75,
            allow_forced_invalidation: true,
        },
        capabilities: CachePolicy::default_capabilities(),
        other: CachePolicy::default_other(),
    };
    client.set_cache_policy_set(&custom);

    let retrieved = client.get_cache_policy_set();
    assert_eq!(retrieved.metadata.min_ttl_seconds, 180);
    assert_eq!(retrieved.metadata.max_ttl_seconds, 10_800);
    assert_eq!(retrieved.metadata.refresh_threshold_pct, 75);
}
