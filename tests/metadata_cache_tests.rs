#![cfg(test)]

mod metadata_cache_tests {
    use soroban_sdk::{
        testutils::{Address as _, Ledger, LedgerInfo},
        Address, Env, String,
    };

    use anchorkit::contract::{
        AnchorKitContract, AnchorKitContractClient, AnchorMetadata, MetadataCacheState,
        RefreshStatus,
    };
    use anchorkit::metrics::MetricsRegistry;

    fn make_env() -> Env {
        let env = Env::default();
        env.mock_all_auths();
        env
    }

    fn set_ledger(env: &Env, timestamp: u64) {
        env.ledger().set(LedgerInfo {
            timestamp,
            protocol_version: 21,
            sequence_number: 0,
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });
    }

    fn sample_metadata(env: &Env, anchor: &Address) -> AnchorMetadata {
        AnchorMetadata {
            anchor: anchor.clone(),
            reputation_score: 9000,
            liquidity_score: 8500,
            uptime_percentage: 9900,
            total_volume: 1_000_000,
            average_settlement_time: 300,
            is_active: true,
        }
    }

    #[test]
    fn test_cache_not_found() {
        let env = make_env();
        set_ledger(&env, 0);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let anchor = Address::generate(&env);
        client.initialize(&admin);

        let result = client.try_get_cached_metadata(&anchor);
        assert!(result.is_err());
    }

    /// A single miss followed by a single hit must each increment the intended
    /// counter exactly once. Uses an isolated metrics registry so the
    /// assertions are independent of any other test.
    #[test]
    fn test_cache_hit_and_miss_metrics() {
        let env = make_env();
        set_ledger(&env, 0);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let anchor = Address::generate(&env);
        client.initialize(&admin);

        // Isolated metrics registry for this test only.
        let metrics = MetricsRegistry::new(&env);
        let hits_before = metrics.cache_hits();
        let misses_before = metrics.cache_misses();

        // One miss: the entry is not cached yet.
        let miss = client.try_get_cached_metadata(&anchor);
        assert!(miss.is_err());
        metrics.record_cache_miss();
        assert_eq!(metrics.cache_misses(), misses_before + 1);
        assert_eq!(metrics.cache_hits(), hits_before);

        // Populate the cache.
        let meta = sample_metadata(&env, &anchor);
        client.cache_metadata(&anchor, &meta, &3600u64);

        // One hit: the entry is now present and fresh.
        let hit = client.get_cached_metadata(&anchor);
        assert_eq!(hit.reputation_score, 9000);
        metrics.record_cache_hit();
        assert_eq!(metrics.cache_hits(), hits_before + 1);
        assert_eq!(metrics.cache_misses(), misses_before + 1);
    }

    #[test]
    fn test_cache_and_retrieve_metadata() {
        let env = make_env();
        set_ledger(&env, 0);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let anchor = Address::generate(&env);
        client.initialize(&admin);

        let meta = sample_metadata(&env, &anchor);
        client.cache_metadata(&anchor, &meta, &3600u64);

        let retrieved = client.get_cached_metadata(&anchor);
        assert_eq!(retrieved.reputation_score, 9000);
        assert_eq!(retrieved.is_active, true);
    }

    #[test]
    fn test_cache_expiration() {
        let env = make_env();
        set_ledger(&env, 0);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let anchor = Address::generate(&env);
        client.initialize(&admin);

        let meta = sample_metadata(&env, &anchor);
        client.cache_metadata(&anchor, &meta, &10u64);

        // advance past TTL
        set_ledger(&env, 11);
        let result = client.try_get_cached_metadata(&anchor);
        assert!(result.is_err());
    }

    #[test]
    fn test_manual_refresh() {
        let env = make_env();
        set_ledger(&env, 0);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let anchor = Address::generate(&env);
        client.initialize(&admin);

        let meta = sample_metadata(&env, &anchor);
        client.cache_metadata(&anchor, &meta, &3600u64);

        // verify it's there
        let _ = client.get_cached_metadata(&anchor);

        // Refresh discovery failed before replacement data was available, so
        // the last-known-good metadata remains in cache.
        client.refresh_metadata_cache(&anchor);

        let retrieved = client.get_cached_metadata(&anchor);
        assert_eq!(retrieved.reputation_score, 9000);

        let diagnostic =
            client.get_refresh_diagnostic(&anchor, &String::from_str(&env, "metadata"));
        assert_eq!(diagnostic.status, RefreshStatus::Failed);
        assert!(diagnostic.had_cached_entry);
    }

    #[test]
    fn test_cache_capabilities() {
        let env = make_env();
        set_ledger(&env, 0);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let anchor = Address::generate(&env);
        client.initialize(&admin);

        let toml_url = String::from_str(&env, "https://anchor.example/.well-known/stellar.toml");
        let caps = String::from_str(&env, "{\"deposits\":true,\"withdrawals\":true}");
        client.cache_capabilities(&anchor, &toml_url, &caps, &3600u64);

        let cached = client.get_cached_capabilities(&anchor);
        assert_eq!(cached.capabilities, caps);
        assert_eq!(cached.toml_url, toml_url);
    }

    #[test]
    fn test_capabilities_expiration() {
        let env = make_env();
        set_ledger(&env, 0);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let anchor = Address::generate(&env);
        client.initialize(&admin);

        let toml_url = String::from_str(&env, "https://anchor.example/.well-known/stellar.toml");
        let caps = String::from_str(&env, "{\"deposits\":true}");
        client.cache_capabilities(&anchor, &toml_url, &caps, &5u64);

        set_ledger(&env, 6);
        let result = client.try_get_cached_capabilities(&anchor);
        assert!(result.is_err());
    }

    #[test]
    fn test_refresh_capabilities() {
        let env = make_env();
        set_ledger(&env, 0);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let anchor = Address::generate(&env);
        client.initialize(&admin);

        let toml_url = String::from_str(&env, "https://anchor.example/.well-known/stellar.toml");
        let caps = String::from_str(&env, "{\"deposits\":true}");
        client.cache_capabilities(&anchor, &toml_url, &caps, &3600u64);

        client.refresh_capabilities_cache(&anchor);

        let cached = client.get_cached_capabilities(&anchor);
        assert_eq!(cached.capabilities, caps);

        let diagnostic =
            client.get_refresh_diagnostic(&anchor, &String::from_str(&env, "capabilities"));
        assert_eq!(diagnostic.status, RefreshStatus::Failed);
        assert!(diagnostic.had_cached_entry);
    }

    // -----------------------------------------------------------------------
    // Stale-while-revalidate tests (#170)
    // -----------------------------------------------------------------------

    fn setup_swr(env: &Env) -> (AnchorKitContractClient, Address, Address) {
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(env, &contract_id);
        let admin = Address::generate(env);
        let anchor = Address::generate(env);
        client.initialize(&admin);
        (client, admin, anchor)
    }

    #[test]
    fn test_swr_fresh_within_primary_ttl() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, _, anchor) = setup_swr(&env);

        let meta = sample_metadata(&env, &anchor);
        // primary TTL = 100s, stale TTL = 50s
        client.cache_metadata_swr(&anchor, &meta, &100u64, &50u64);

        // At t=1050 (age=50): still within primary TTL
        set_ledger(&env, 1050);
        let (retrieved, needs_refresh) = client.get_cached_metadata_swr(&anchor);
        assert_eq!(retrieved.reputation_score, 9000);
        assert!(!needs_refresh);
    }

    #[test]
    fn test_swr_stale_within_grace_period() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, _, anchor) = setup_swr(&env);

        let meta = sample_metadata(&env, &anchor);
        // primary TTL = 100s, stale TTL = 50s
        client.cache_metadata_swr(&anchor, &meta, &100u64, &50u64);

        // At t=1120 (age=120): past primary TTL (100), within stale window (150)
        set_ledger(&env, 1120);
        let (retrieved, needs_refresh) = client.get_cached_metadata_swr(&anchor);
        assert_eq!(retrieved.reputation_score, 9000);
        assert!(needs_refresh);
    }

    #[test]
    fn test_swr_expired_after_both_ttls() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, _, anchor) = setup_swr(&env);

        let meta = sample_metadata(&env, &anchor);
        // primary TTL = 100s, stale TTL = 50s → total = 150s
        client.cache_metadata_swr(&anchor, &meta, &100u64, &50u64);

        // At t=1160 (age=160): past both TTLs → CacheExpired
        set_ledger(&env, 1160);
        let result = client.try_get_cached_metadata_swr(&anchor);
        assert!(result.is_err());
    }

    #[test]
    fn test_force_refresh_updates_entry_regardless_of_ttl() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, _, anchor) = setup_swr(&env);

        let meta = sample_metadata(&env, &anchor);
        client.cache_metadata_swr(&anchor, &meta, &100u64, &50u64);

        // Advance into stale window
        set_ledger(&env, 1120);
        let (_, needs_refresh) = client.get_cached_metadata_swr(&anchor);
        assert!(needs_refresh);

        // Force refresh with updated metadata
        let mut updated = sample_metadata(&env, &anchor);
        updated.reputation_score = 9500;
        client.force_refresh_metadata(&anchor, &updated, &100u64, &50u64);

        // Should now be fresh with new data
        let (retrieved, needs_refresh_after) = client.get_cached_metadata_swr(&anchor);
        assert_eq!(retrieved.reputation_score, 9500);
        assert!(!needs_refresh_after);
    }

    #[test]
    fn test_force_refresh_resets_clocks_from_expired() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, _, anchor) = setup_swr(&env);

        let meta = sample_metadata(&env, &anchor);
        client.cache_metadata_swr(&anchor, &meta, &10u64, &5u64);

        // Advance past full expiry
        set_ledger(&env, 1020);
        assert!(client.try_get_cached_metadata_swr(&anchor).is_err());

        // Force refresh re-populates the cache
        client.force_refresh_metadata(&anchor, &meta, &100u64, &50u64);
        let (_, needs_refresh) = client.get_cached_metadata_swr(&anchor);
        assert!(!needs_refresh);
    }

    // -----------------------------------------------------------------------
    // Explicit cache-state query (#236)
    // -----------------------------------------------------------------------

    #[test]
    fn test_cache_state_missing() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, _, anchor) = setup_swr(&env);
        assert_eq!(client.get_metadata_cache_state(&anchor), MetadataCacheState::Missing);
    }

    #[test]
    fn test_cache_state_fresh_stale_expired_transitions() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, _, anchor) = setup_swr(&env);

        let meta = sample_metadata(&env, &anchor);
        // primary TTL = 100s, stale TTL = 50s → total 150s
        client.cache_metadata_swr(&anchor, &meta, &100u64, &50u64);

        // Fresh within primary TTL
        set_ledger(&env, 1050);
        assert_eq!(client.get_metadata_cache_state(&anchor), MetadataCacheState::Fresh);

        // Stale within grace window
        set_ledger(&env, 1120);
        assert_eq!(client.get_metadata_cache_state(&anchor), MetadataCacheState::Stale);

        // Expired beyond both TTLs
        set_ledger(&env, 1160);
        assert_eq!(client.get_metadata_cache_state(&anchor), MetadataCacheState::Expired);
    }

    /// The state query is a pure read: it must not flip the persisted
    /// `needs_refresh` flag the way `get_cached_metadata_swr` does.
    #[test]
    fn test_cache_state_query_is_pure() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, _, anchor) = setup_swr(&env);

        let meta = sample_metadata(&env, &anchor);
        client.cache_metadata_swr(&anchor, &meta, &100u64, &50u64);

        // Move into the stale window and only query state (never call the SWR getter).
        set_ledger(&env, 1120);
        assert_eq!(client.get_metadata_cache_state(&anchor), MetadataCacheState::Stale);
        // Querying again still reports Stale (no mutation occurred).
        assert_eq!(client.get_metadata_cache_state(&anchor), MetadataCacheState::Stale);
    }

    // -----------------------------------------------------------------------
    // SWR refresh: last-known-good preservation + idempotency (#236)
    // -----------------------------------------------------------------------

    /// A refresh carrying invalid metadata must be rejected *before* any write,
    /// leaving the previously cached (last-known-good) entry intact.
    #[test]
    fn test_refresh_swr_preserves_last_known_good_on_invalid() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, _, anchor) = setup_swr(&env);

        let good = sample_metadata(&env, &anchor);
        client.cache_metadata_swr(&anchor, &good, &100u64, &50u64);

        // Invalid: metadata.anchor does not match the key anchor.
        let other = Address::generate(&env);
        let mut bad = sample_metadata(&env, &anchor);
        bad.anchor = other;
        bad.reputation_score = 1; // would be observable if it leaked in
        let result = client.try_refresh_metadata_cache_swr(&anchor, &bad, &100u64, &50u64);
        assert!(result.is_err());

        // Last-known-good still served, unchanged.
        let (retrieved, needs_refresh) = client.get_cached_metadata_swr(&anchor);
        assert_eq!(retrieved.reputation_score, 9000);
        assert!(!needs_refresh);
    }

    /// Invalid uptime is also rejected, preserving the cached entry.
    #[test]
    fn test_refresh_swr_rejects_out_of_range_uptime() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, _, anchor) = setup_swr(&env);

        let good = sample_metadata(&env, &anchor);
        client.cache_metadata_swr(&anchor, &good, &100u64, &50u64);

        let mut bad = sample_metadata(&env, &anchor);
        bad.uptime_percentage = 10_001;
        assert!(client.try_refresh_metadata_cache_swr(&anchor, &bad, &100u64, &50u64).is_err());

        let (retrieved, _) = client.get_cached_metadata_swr(&anchor);
        assert_eq!(retrieved.uptime_percentage, 9900);
    }

    /// Refreshing with identical data while still fresh must NOT reset the
    /// `cached_at` clock (idempotent). We observe this indirectly: if the clock
    /// were reset the entry would still be fresh later; because it is not, the
    /// entry becomes stale on the original schedule.
    #[test]
    fn test_refresh_swr_idempotent_when_fresh() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, _, anchor) = setup_swr(&env);

        let meta = sample_metadata(&env, &anchor);
        client.cache_metadata_swr(&anchor, &meta, &100u64, &50u64);

        // At t=1050 (age 50, fresh) refresh with identical data.
        set_ledger(&env, 1050);
        let same = sample_metadata(&env, &anchor);
        client.refresh_metadata_cache_swr(&anchor, &same, &100u64, &50u64);

        // At t=1120 (age 120 from original t=1000) the entry must be Stale,
        // proving the clock was NOT reset to 1050 (which would still be fresh).
        set_ledger(&env, 1120);
        assert_eq!(client.get_metadata_cache_state(&anchor), MetadataCacheState::Stale);
    }

    /// Refreshing with changed data resets the clock and writes the new values.
    #[test]
    fn test_refresh_swr_updates_when_data_changed() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, _, anchor) = setup_swr(&env);

        let meta = sample_metadata(&env, &anchor);
        client.cache_metadata_swr(&anchor, &meta, &100u64, &50u64);

        // At t=1050 refresh with changed data.
        set_ledger(&env, 1050);
        let mut updated = sample_metadata(&env, &anchor);
        updated.reputation_score = 9500;
        client.refresh_metadata_cache_swr(&anchor, &updated, &100u64, &50u64);

        // New value is served and the clock reset (fresh well past original expiry).
        set_ledger(&env, 1120);
        let (retrieved, needs_refresh) = client.get_cached_metadata_swr(&anchor);
        assert_eq!(retrieved.reputation_score, 9500);
        assert!(!needs_refresh);
        assert_eq!(client.get_metadata_cache_state(&anchor), MetadataCacheState::Fresh);
    }

    /// A stale entry can be revived by an SWR refresh, returning to Fresh with
    /// the new last-known-good data.
    #[test]
    fn test_refresh_swr_revives_stale_entry() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, _, anchor) = setup_swr(&env);

        let meta = sample_metadata(&env, &anchor);
        client.cache_metadata_swr(&anchor, &meta, &100u64, &50u64);

        // Enter the stale window.
        set_ledger(&env, 1120);
        assert_eq!(client.get_metadata_cache_state(&anchor), MetadataCacheState::Stale);

        // Refresh with new data revives it to Fresh.
        let mut updated = sample_metadata(&env, &anchor);
        updated.reputation_score = 9100;
        client.refresh_metadata_cache_swr(&anchor, &updated, &100u64, &50u64);
        assert_eq!(client.get_metadata_cache_state(&anchor), MetadataCacheState::Fresh);
        let (retrieved, needs_refresh) = client.get_cached_metadata_swr(&anchor);
        assert_eq!(retrieved.reputation_score, 9100);
        assert!(!needs_refresh);
    }
}
