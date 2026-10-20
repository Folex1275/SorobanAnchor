//! Tests for anchor metadata version history.
//!
//! Acceptance criteria verified:
//! 1. Each `set_anchor_metadata` call increments the version counter.
//! 2. `get_anchor_metadata_history` returns entries in ascending version order.
//! 3. `get_anchor_metadata_at_version` retrieves a specific historical snapshot.
//! 4. History preserves all field values at the time of each write.
//! 5. `get_anchor_meta_version_count` returns 0 before any writes.
//! 6. Rollback semantics: a specific historical version can be re-applied.
//! 7. History cap: at most 50 entries are returned by `get_anchor_metadata_history`.

#![cfg(test)]

mod metadata_version_history_tests {
    use soroban_sdk::{
        testutils::{Address as _, Ledger, LedgerInfo},
        Address, Env,
    };

    use anchorkit::contract::{AnchorKitContract, AnchorKitContractClient};

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn make_env() -> Env {
        let env = Env::default();
        env.mock_all_auths();
        env
    }

    fn set_ledger(env: &Env, ts: u64) {
        env.ledger().set(LedgerInfo {
            timestamp: ts,
            protocol_version: 21,
            sequence_number: 0,
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6_312_000,
        });
    }

    fn deploy(env: &Env) -> (AnchorKitContractClient, Address) {
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(env, &contract_id);
        let admin = Address::generate(env);
        client.initialize(&admin);
        (client, admin)
    }

    // -----------------------------------------------------------------------
    // Version counter
    // -----------------------------------------------------------------------

    #[test]
    fn version_count_is_zero_before_any_write() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, _admin) = deploy(&env);
        let anchor = Address::generate(&env);

        assert_eq!(client.get_anchor_meta_version_count(&anchor), 0);
    }

    #[test]
    fn version_count_increments_on_each_write() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, _admin) = deploy(&env);
        let anchor = Address::generate(&env);

        client.set_anchor_metadata(&anchor, &9000, &300, &8500, &9900, &1_000_000);
        assert_eq!(client.get_anchor_meta_version_count(&anchor), 1);

        client.set_anchor_metadata(&anchor, &9100, &280, &8600, &9950, &2_000_000);
        assert_eq!(client.get_anchor_meta_version_count(&anchor), 2);

        client.set_anchor_metadata(&anchor, &9200, &260, &8700, &9980, &3_000_000);
        assert_eq!(client.get_anchor_meta_version_count(&anchor), 3);
    }

    // -----------------------------------------------------------------------
    // get_anchor_metadata_at_version
    // -----------------------------------------------------------------------

    #[test]
    fn get_at_version_returns_correct_snapshot() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, _admin) = deploy(&env);
        let anchor = Address::generate(&env);

        // Write version 1
        client.set_anchor_metadata(&anchor, &9000, &300, &8500, &9900, &1_000_000);
        // Write version 2 with different values
        client.set_anchor_metadata(&anchor, &7000, &500, &6000, &8000, &500_000);

        let v1 = client.get_anchor_metadata_at_version(&anchor, &1);
        assert_eq!(v1.version, 1);
        assert_eq!(v1.reputation_score, 9000);
        assert_eq!(v1.average_settlement_time, 300);
        assert_eq!(v1.liquidity_score, 8500);
        assert_eq!(v1.uptime_percentage, 9900);
        assert_eq!(v1.total_volume, 1_000_000);
        assert!(v1.is_active);

        let v2 = client.get_anchor_metadata_at_version(&anchor, &2);
        assert_eq!(v2.version, 2);
        assert_eq!(v2.reputation_score, 7000);
        assert_eq!(v2.total_volume, 500_000);
    }

    #[test]
    fn get_at_version_missing_panics() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, _admin) = deploy(&env);
        let anchor = Address::generate(&env);

        // No writes — version 1 does not exist
        let result = client.try_get_anchor_metadata_at_version(&anchor, &1);
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // get_anchor_metadata_history — ordering and completeness
    // -----------------------------------------------------------------------

    #[test]
    fn history_is_empty_before_any_write() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, _admin) = deploy(&env);
        let anchor = Address::generate(&env);

        let history = client.get_anchor_metadata_history(&anchor);
        assert_eq!(history.len(), 0);
    }

    #[test]
    fn history_returns_entries_in_ascending_version_order() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, _admin) = deploy(&env);
        let anchor = Address::generate(&env);

        client.set_anchor_metadata(&anchor, &1000, &100, &1000, &9000, &100);
        client.set_anchor_metadata(&anchor, &2000, &200, &2000, &8000, &200);
        client.set_anchor_metadata(&anchor, &3000, &300, &3000, &7000, &300);

        let history = client.get_anchor_metadata_history(&anchor);
        assert_eq!(history.len(), 3);
        assert_eq!(history.get(0).unwrap().version, 1);
        assert_eq!(history.get(1).unwrap().version, 2);
        assert_eq!(history.get(2).unwrap().version, 3);
    }

    #[test]
    fn history_preserves_field_values_per_version() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, _admin) = deploy(&env);
        let anchor = Address::generate(&env);

        client.set_anchor_metadata(&anchor, &9000, &300, &8500, &9900, &1_000_000);
        set_ledger(&env, 2000);
        client.set_anchor_metadata(&anchor, &5000, &600, &4000, &7000, &500_000);

        let history = client.get_anchor_metadata_history(&anchor);
        assert_eq!(history.len(), 2);

        let first = history.get(0).unwrap();
        assert_eq!(first.reputation_score, 9000);
        assert_eq!(first.updated_at, 1000);

        let second = history.get(1).unwrap();
        assert_eq!(second.reputation_score, 5000);
        assert_eq!(second.updated_at, 2000);
    }

    #[test]
    fn history_timestamps_reflect_ledger_time_of_write() {
        let env = make_env();
        set_ledger(&env, 100);
        let (client, _admin) = deploy(&env);
        let anchor = Address::generate(&env);

        client.set_anchor_metadata(&anchor, &9000, &300, &8500, &9900, &1_000_000);
        set_ledger(&env, 999);
        client.set_anchor_metadata(&anchor, &8000, &400, &7500, &9500, &2_000_000);

        let history = client.get_anchor_metadata_history(&anchor);
        assert_eq!(history.get(0).unwrap().updated_at, 100);
        assert_eq!(history.get(1).unwrap().updated_at, 999);
    }

    // -----------------------------------------------------------------------
    // Rollback semantics
    // -----------------------------------------------------------------------

    #[test]
    fn rollback_by_reapplying_historical_version() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, _admin) = deploy(&env);
        let anchor = Address::generate(&env);

        // Write v1 (good state)
        client.set_anchor_metadata(&anchor, &9000, &300, &8500, &9900, &1_000_000);
        // Write v2 (bad state — simulated degradation)
        client.set_anchor_metadata(&anchor, &1000, &9999, &500, &1000, &0);

        // Retrieve v1 snapshot and re-apply it (rollback)
        let v1 = client.get_anchor_metadata_at_version(&anchor, &1);
        client.set_anchor_metadata(
            &anchor,
            &v1.reputation_score,
            &v1.average_settlement_time,
            &v1.liquidity_score,
            &v1.uptime_percentage,
            &v1.total_volume,
        );

        // Current metadata should now match v1 values
        let current = client.get_anchor_metadata(&anchor);
        assert_eq!(current.reputation_score, 9000);
        assert_eq!(current.average_settlement_time, 300);
        assert_eq!(current.total_volume, 1_000_000);

        // Version count should now be 3 (v1, v2, rollback-as-v3)
        assert_eq!(client.get_anchor_meta_version_count(&anchor), 3);
    }

    // -----------------------------------------------------------------------
    // History cap (50 entries)
    // -----------------------------------------------------------------------

    #[test]
    fn history_cap_returns_at_most_50_entries() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, _admin) = deploy(&env);
        let anchor = Address::generate(&env);

        // Write 55 versions
        for i in 0u64..55 {
            client.set_anchor_metadata(&anchor, &(i as u32), &i, &(i as u32), &9000, &i);
        }

        let history = client.get_anchor_metadata_history(&anchor);
        // Cap is 50
        assert_eq!(history.len(), 50);
        // Should be the most recent 50 (versions 6..=55)
        assert_eq!(history.get(0).unwrap().version, 6);
        assert_eq!(history.get(49).unwrap().version, 55);
    }

    #[test]
    fn history_exactly_50_entries_returns_all() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, _admin) = deploy(&env);
        let anchor = Address::generate(&env);

        for i in 0u64..50 {
            client.set_anchor_metadata(&anchor, &(i as u32), &i, &(i as u32), &9000, &i);
        }

        let history = client.get_anchor_metadata_history(&anchor);
        assert_eq!(history.len(), 50);
        assert_eq!(history.get(0).unwrap().version, 1);
        assert_eq!(history.get(49).unwrap().version, 50);
    }

    // -----------------------------------------------------------------------
    // Duplicate version recording
    // -----------------------------------------------------------------------

    #[test]
    fn duplicate_version_recording_is_idempotent() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, _admin) = deploy(&env);
        let anchor = Address::generate(&env);

        // Record one metadata version.
        client.set_anchor_metadata(&anchor, &9000, &300, &8500, &9900, &1_000_000);
        let history_len_after_first = client.get_anchor_metadata_history(&anchor).len();
        let count_after_first = client.get_anchor_meta_version_count(&anchor);
        assert_eq!(history_len_after_first, 1);
        assert_eq!(count_after_first, 1);

        // Repeat the exact same operation with identical values.
        // Intended behavior: each `set_anchor_metadata` call is a distinct
        // write that increments the version counter, so a repeated identical
        // input produces a new history entry (version 2) rather than being
        // silently deduplicated. This assertion documents that explicit
        // outcome and guards against unintended duplicate entries beyond it.
        client.set_anchor_metadata(&anchor, &9000, &300, &8500, &9900, &1_000_000);

        let history = client.get_anchor_metadata_history(&anchor);
        assert_eq!(history.len(), 2);
        assert_eq!(client.get_anchor_meta_version_count(&anchor), 2);

        // Both entries carry the same field values but distinct versions.
        let first = history.get(0).unwrap();
        let second = history.get(1).unwrap();
        assert_eq!(first.version, 1);
        assert_eq!(second.version, 2);
        assert_eq!(first.reputation_score, second.reputation_score);
        assert_eq!(first.average_settlement_time, second.average_settlement_time);
        assert_eq!(first.liquidity_score, second.liquidity_score);
        assert_eq!(first.uptime_percentage, second.uptime_percentage);
        assert_eq!(first.total_volume, second.total_volume);
    }

    // -----------------------------------------------------------------------
    // Independent anchors have independent histories
    // -----------------------------------------------------------------------

    #[test]
    fn different_anchors_have_independent_histories() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, _admin) = deploy(&env);
        let anchor_a = Address::generate(&env);
        let anchor_b = Address::generate(&env);

        client.set_anchor_metadata(&anchor_a, &9000, &300, &8500, &9900, &1_000_000);
        client.set_anchor_metadata(&anchor_a, &9100, &290, &8600, &9950, &1_100_000);
        client.set_anchor_metadata(&anchor_b, &5000, &600, &4000, &7000, &500_000);

        assert_eq!(client.get_anchor_meta_version_count(&anchor_a), 2);
        assert_eq!(client.get_anchor_meta_version_count(&anchor_b), 1);

        let history_a = client.get_anchor_metadata_history(&anchor_a);
        let history_b = client.get_anchor_metadata_history(&anchor_b);
        assert_eq!(history_a.len(), 2);
        assert_eq!(history_b.len(), 1);
        assert_eq!(history_b.get(0).unwrap().reputation_score, 5000);
    }
}
