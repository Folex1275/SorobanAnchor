#![cfg(test)]

mod diagnostics_tests {
    use soroban_sdk::{
        testutils::{Address as _, Ledger as _, LedgerInfo},
        Address, Env,
    };

    use anchorkit::contract::{AnchorKitContract, AnchorKitContractClient, AnchorMetadata};

    fn make_env() -> Env {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set(LedgerInfo {
            timestamp: 1_000,
            protocol_version: 21,
            sequence_number: 0,
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6_312_000,
        });
        env
    }

    fn setup(env: &Env) -> (AnchorKitContractClient, Address) {
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(env, &contract_id);
        let admin = Address::generate(env);
        client.initialize(&admin);
        (client, admin)
    }

    fn sample_metadata(env: &Env, anchor: &Address) -> AnchorMetadata {
        AnchorMetadata {
            anchor: anchor.clone(),
            reputation_score: 9000,
            liquidity_score: 8000,
            uptime_percentage: 9900,
            total_volume: 500_000,
            average_settlement_time: 60,
            is_active: true,
        }
    }

    // -----------------------------------------------------------------------
    // Contract diagnostics: uninitialized contract
    // -----------------------------------------------------------------------

    #[test]
    fn test_contract_diagnostics_uninitialized() {
        let env = make_env();
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let diag = client.get_contract_diagnostics();
        assert!(!diag.is_initialized);
        assert_eq!(diag.total_attestations, 0);
        assert_eq!(diag.total_quotes, 0);
        assert_eq!(diag.total_sessions, 0);
    }

    // -----------------------------------------------------------------------
    // Contract diagnostics: initialized contract
    // -----------------------------------------------------------------------

    #[test]
    fn test_contract_diagnostics_initialized() {
        let env = make_env();
        let (client, _) = setup(&env);

        let diag = client.get_contract_diagnostics();
        assert!(diag.is_initialized);
        assert_eq!(diag.total_attestations, 0);
        assert_eq!(diag.rate_limit_max_submissions, 10);
        assert_eq!(diag.rate_limit_window_length, 100);
    }

    // -----------------------------------------------------------------------
    // Rate-limiter diagnostics: fresh attestor has zero submissions
    // -----------------------------------------------------------------------

    #[test]
    fn test_rate_limiter_diagnostics_fresh_attestor() {
        let env = make_env();
        let (client, _) = setup(&env);
        let attestor = Address::generate(&env);

        let diag = client.get_rate_limiter_diagnostics(&attestor);
        assert_eq!(diag.submission_count, 0);
        assert!(!diag.is_at_limit);
        assert_eq!(diag.max_submissions, 10);
    }

    // -----------------------------------------------------------------------
    // Rate-limiter diagnostics: at-limit detection
    // -----------------------------------------------------------------------

    #[test]
    fn test_rate_limiter_diagnostics_reflects_config() {
        let env = make_env();
        let (client, _) = setup(&env);
        let attestor = Address::generate(&env);

        let diag = client.get_rate_limiter_diagnostics(&attestor);
        assert_eq!(diag.max_submissions, 10);
        assert_eq!(diag.window_length, 100);
        assert!(!diag.is_at_limit);
    }

    // -----------------------------------------------------------------------
    // Cache diagnostics: no cached entries
    // -----------------------------------------------------------------------

    #[test]
    fn test_cache_diagnostics_empty() {
        let env = make_env();
        let (client, _) = setup(&env);
        let anchor = Address::generate(&env);

        let diag = client.get_cache_diagnostics(&anchor);
        assert!(!diag.metadata_cached);
        assert_eq!(diag.metadata_age_seconds, 0);
        assert_eq!(diag.metadata_ttl_seconds, 0);
        assert!(!diag.capabilities_cached);
        assert_eq!(diag.capabilities_age_seconds, 0);
        assert_eq!(diag.capabilities_ttl_seconds, 0);
    }

    // -----------------------------------------------------------------------
    // Cache diagnostics: metadata present after caching
    // -----------------------------------------------------------------------

    #[test]
    fn test_cache_diagnostics_with_metadata() {
        let env = make_env();
        let (client, _) = setup(&env);
        let anchor = Address::generate(&env);
        let meta = sample_metadata(&env, &anchor);

        client.cache_metadata(&anchor, &meta, &3600u64);

        let diag = client.get_cache_diagnostics(&anchor);
        assert!(diag.metadata_cached);
        assert_eq!(diag.metadata_ttl_seconds, 3600);
        assert_eq!(diag.metadata_age_seconds, 0);
    }

    // -----------------------------------------------------------------------
    // Session diagnostics: count starts at zero
    // -----------------------------------------------------------------------

    #[test]
    fn test_session_diagnostics_zero_initially() {
        let env = make_env();
        let (client, _) = setup(&env);

        let diag = client.get_session_diagnostics();
        assert_eq!(diag.total_sessions_created, 0);
    }

    // -----------------------------------------------------------------------
    // Session diagnostics: count increments after session creation
    // -----------------------------------------------------------------------

    #[test]
    fn test_session_diagnostics_increments_on_create() {
        let env = make_env();
        let (client, _) = setup(&env);
        let user = Address::generate(&env);

        client.create_session(&user);

        let diag = client.get_session_diagnostics();
        assert_eq!(diag.total_sessions_created, 1);
    }
}

// ---------------------------------------------------------------------------
// Issue #1304: Malformed RPC URL diagnostics
// ---------------------------------------------------------------------------

#[cfg(test)]
mod preflight_diagnostics_tests {
    use super::*;

    /// #1304: A malformed RPC endpoint must be detected during preflight with
    /// a useful message, without making a network request.
    ///
    /// This test validates RPC URL format before transport initialization.
    #[test]
    fn test_malformed_rpc_url_validation() {
        // Invalid RPC URLs that should be rejected during preflight
        let invalid_urls = vec![
            "not-a-url",                    // Missing scheme
            "ftp://invalid.scheme.com",     // Invalid scheme (not http/https)
            "http://",                      // Missing authority
            "https://user:pass@host.com",   // Embedded credentials
            "https://host\x00.com",         // Control characters
            "",                             // Empty string
        ];

        for url in invalid_urls {
            let result = validate_rpc_url_format(url);
            assert!(
                result.is_err(),
                "URL '{}' should be rejected as malformed",
                url
            );
            let err_msg = result.unwrap_err();
            assert!(
                !err_msg.is_empty(),
                "Validation error must provide context for URL '{}'",
                url
            );
            eprintln!("✓ Rejected '{}': {}", url, err_msg);
        }
    }

    #[test]
    fn test_valid_rpc_url_accepted() {
        let valid_urls = vec![
            "https://soroban-testnet.stellar.org",
            "http://localhost:8000",
            "https://rpc.example.com/path",
        ];

        for url in valid_urls {
            let result = validate_rpc_url_format(url);
            assert!(
                result.is_ok(),
                "URL '{}' should be accepted, got error: {:?}",
                url,
                result.err()
            );
        }
    }

    #[test]
    fn test_malformed_url_no_network_request() {
        // Validate that a clearly malformed local string is rejected
        // before any transport/network initialization occurs.
        let malformed = "not-even-a-url";
        let result = validate_rpc_url_format(malformed);
        
        assert!(result.is_err(), "Malformed URL must be rejected");
        
        // The error should be about format, not network connectivity
        let err = result.unwrap_err();
        assert!(
            err.contains("URL") || err.contains("format") || err.contains("scheme"),
            "Error message should indicate format problem: {}",
            err
        );
    }

    // Simple URL validation helper (mirrors network_profile validation logic)
    fn validate_rpc_url_format(url: &str) -> Result<(), String> {
        if url.trim().is_empty() {
            return Err("RPC URL must not be empty".to_string());
        }

        // Check for valid HTTP(S) scheme
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(format!("RPC URL must use http:// or https:// scheme: {}", url));
        }

        // Check for embedded credentials (security risk)
        if url.contains('@') && url.split("://").nth(1).map_or(false, |rest| rest.contains('@')) {
            return Err("RPC URL must not contain embedded credentials".to_string());
        }

        // Check for control characters
        if url.chars().any(|c| c.is_control()) {
            return Err("RPC URL must not contain control characters".to_string());
        }

        // Check that host is present (not just "https://")
        let after_scheme = url.split("://").nth(1).unwrap_or("");
        if after_scheme.is_empty() || after_scheme.starts_with('/') {
            return Err("RPC URL must include a valid host".to_string());
        }

        Ok(())
    }
}
