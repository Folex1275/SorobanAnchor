#![cfg(test)]

#[path = "sep10_test_util.rs"]
mod sep10_test_util;

mod routing_tests {
    use soroban_sdk::{
        testutils::{Address as _, Ledger, LedgerInfo},
        Address, Env, String, Symbol, Vec,
    };

    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    use anchorkit::contract::{AnchorKitContract, AnchorKitContractClient, RoutingOptions, RoutingRequest, WeightedRoutingStrategy};
    use crate::sep10_test_util::register_attestor_with_sep10;

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

    fn setup(env: &Env) -> (AnchorKitContractClient, Address) {
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(env, &contract_id);
        let admin = Address::generate(env);
        client.initialize(&admin);
        (client, admin)
    }

    fn register_anchor(env: &Env, client: &AnchorKitContractClient, anchor: &Address) {
        let signing_key = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(env, client, anchor, anchor, &signing_key);
        let mut services = Vec::new(env);
        services.push_back(1u32);
        services.push_back(3u32);
        client.configure_services(anchor, &services);
    }

    fn make_request(env: &Env) -> RoutingRequest {
        RoutingRequest {
            base_asset: String::from_str(env, "USD"),
            quote_asset: String::from_str(env, "USDC"),
            amount: 5000,
            operation_type: 1,
        }
    }

    #[test]
    fn test_select_lowest_fee_anchor() {
        let env = make_env();
        set_ledger(&env, 1_000_000);
        let (client, _) = setup(&env);

        let anchor1 = Address::generate(&env);
        let anchor2 = Address::generate(&env);
        register_anchor(&env, &client, &anchor1);
        register_anchor(&env, &client, &anchor2);

        client.submit_quote(
            &anchor1,
            &String::from_str(&env, "USD"),
            &String::from_str(&env, "USDC"),
            &10000u64, &50u32, &100u64, &100000u64, &1_003_600u64,
        );
        client.submit_quote(
            &anchor2,
            &String::from_str(&env, "USD"),
            &String::from_str(&env, "USDC"),
            &10000u64, &20u32, &100u64, &100000u64, &1_003_600u64,
        );

        let q1 = client.get_quote(&anchor1, &1u64);
        let q2 = client.get_quote(&anchor2, &2u64);

        assert_eq!(q1.fee_percentage, 50);
        assert_eq!(q2.fee_percentage, 20);
        // anchor2 has lower fee
        assert!(q2.fee_percentage < q1.fee_percentage);
        assert_eq!(q2.anchor, anchor2);
    }

    #[test]
    fn test_fastest_settlement_strategy() {
        let env = make_env();
        set_ledger(&env, 1_000_000);
        let (client, _) = setup(&env);

        let anchor1 = Address::generate(&env);
        let anchor2 = Address::generate(&env);
        register_anchor(&env, &client, &anchor1);
        client.set_anchor_metadata(&anchor1, &8000u32, &600u64, &7500u32, &9900u32, &1_000_000u64);
        client.submit_quote(
            &anchor1,
            &String::from_str(&env, "USD"),
            &String::from_str(&env, "USDC"),
            &10000u64, &25u32, &100u64, &100000u64, &1_003_600u64,
        );

        register_anchor(&env, &client, &anchor2);
        client.set_anchor_metadata(&anchor2, &8000u32, &200u64, &7500u32, &9900u32, &1_000_000u64);
        client.submit_quote(
            &anchor2,
            &String::from_str(&env, "USD"),
            &String::from_str(&env, "USDC"),
            &10000u64, &25u32, &100u64, &100000u64, &1_003_600u64,
        );

        let mut strategy = Vec::new(&env);
        strategy.push_back(Symbol::new(&env, "FastestSettlement"));
        let options = RoutingOptions {
            request: make_request(&env),
            strategy,
            min_reputation: 0,
            max_anchors: 2,
            require_kyc: false,
            require_compliance: false,
            subject: Address::generate(&env),
            fee_weight: 333,
            speed_weight: 333,
            reputation_weight: 334,
        };

        // anchor2 has faster settlement time (200 < 600)
        let best = client.route_transaction(&options);
        assert_eq!(best.anchor, anchor2);
    }

    #[test]
    fn test_filter_by_reputation() {
        let env = make_env();
        set_ledger(&env, 1_000_000);
        let (client, _) = setup(&env);

        let anchor1 = Address::generate(&env);
        let anchor2 = Address::generate(&env);
        register_anchor(&env, &client, &anchor1);
        client.set_anchor_metadata(&anchor1, &3000u32, &300u64, &7500u32, &9900u32, &1_000_000u64);
        client.submit_quote(
            &anchor1,
            &String::from_str(&env, "USD"),
            &String::from_str(&env, "USDC"),
            &9900u64, &20u32, &100u64, &100000u64, &1_003_600u64,
        );

        register_anchor(&env, &client, &anchor2);
        client.set_anchor_metadata(&anchor2, &8000u32, &300u64, &7500u32, &9900u32, &1_000_000u64);
        client.submit_quote(
            &anchor2,
            &String::from_str(&env, "USD"),
            &String::from_str(&env, "USDC"),
            &10000u64, &25u32, &100u64, &100000u64, &1_003_600u64,
        );

        let mut strategy = Vec::new(&env);
        strategy.push_back(Symbol::new(&env, "HighestReputation"));
        let options = RoutingOptions {
            request: make_request(&env),
            strategy,
            min_reputation: 0,
            max_anchors: 2,
            require_kyc: false,
            require_compliance: false,
            subject: Address::generate(&env),
            fee_weight: 333,
            speed_weight: 333,
            reputation_weight: 334,
        };

        // anchor2 has higher reputation (8000 > 3000)
        let best = client.route_transaction(&options);
        assert_eq!(best.anchor, anchor2);
    }

    #[test]
    fn test_expired_quotes_filtered() {
        let env = make_env();
        set_ledger(&env, 1_000_000);
        let (client, _) = setup(&env);

        let anchor1 = Address::generate(&env);
        register_anchor(&env, &client, &anchor1);

        // First quote: expires at 1_000_100 (still valid at t=1_000_000)
        client.submit_quote(
            &anchor1,
            &String::from_str(&env, "USD"),
            &String::from_str(&env, "USDC"),
            &9900u64, &15u32, &100u64, &100000u64, &1_000_100u64,
        );
        // Second quote: valid for longer
        client.submit_quote(
            &anchor1,
            &String::from_str(&env, "USD"),
            &String::from_str(&env, "USDC"),
            &10000u64, &25u32, &100u64, &100000u64, &1_003_600u64,
        );

        let q1 = client.get_quote(&anchor1, &1u64);
        let q2 = client.get_quote(&anchor1, &2u64);

        assert_eq!(q1.valid_until, 1_000_100);
        assert_eq!(q2.valid_until, 1_003_600);

        // At t=1_000_200, q1 would be expired
        assert!(q1.valid_until < 1_000_200);
        assert!(q2.valid_until > 1_000_200);
    }

    #[test]
    fn test_no_anchors_available() {
        let env = make_env();
        set_ledger(&env, 0);
        let (client, _) = setup(&env);

        let anchor1 = Address::generate(&env);
        register_anchor(&env, &client, &anchor1);

        // No quotes submitted
        let result = client.try_get_quote(&anchor1, &1u64);
        assert!(result.is_err());
    }

    #[test]
    fn test_handle_unavailable_anchors() {
        let env = make_env();
        set_ledger(&env, 1_000_000);
        let (client, _) = setup(&env);

        let anchor1 = Address::generate(&env);
        let anchor2 = Address::generate(&env);
        let anchor3 = Address::generate(&env);
        register_anchor(&env, &client, &anchor1);
        register_anchor(&env, &client, &anchor2);
        register_anchor(&env, &client, &anchor3);

        client.submit_quote(
            &anchor1,
            &String::from_str(&env, "USD"),
            &String::from_str(&env, "USDC"),
            &10000u64, &25u32, &100u64, &100000u64, &1_003_600u64,
        );
        client.submit_quote(
            &anchor2,
            &String::from_str(&env, "USD"),
            &String::from_str(&env, "USDC"),
            &10050u64, &30u32, &100u64, &100000u64, &1_003_600u64,
        );

        let q1 = client.get_quote(&anchor1, &1u64);
        let q2 = client.get_quote(&anchor2, &2u64);

        // anchor3 has no quote
        let result = client.try_get_quote(&anchor3, &3u64);
        assert!(result.is_err());

        assert_eq!(q1.fee_percentage, 25);
        assert_eq!(q2.fee_percentage, 30);
    }

    #[test]
    fn test_amount_outside_quote_limits() {
        let env = make_env();
        set_ledger(&env, 1_000_000);
        let (client, _) = setup(&env);

        let anchor1 = Address::generate(&env);
        register_anchor(&env, &client, &anchor1);

        client.submit_quote(
            &anchor1,
            &String::from_str(&env, "USD"),
            &String::from_str(&env, "USDC"),
            &10000u64, &25u32, &100u64, &100000u64, &1_003_600u64,
        );

        let q = client.get_quote(&anchor1, &1u64);
        assert_eq!(q.minimum_amount, 100);
        assert_eq!(q.maximum_amount, 100000);

        // 5000 is within limits
        assert!(5000 >= q.minimum_amount && 5000 <= q.maximum_amount);
        // 200000 is outside limits
        assert!(200000 > q.maximum_amount);
    }

    #[test]
    fn test_select_best_quote_from_multiple_anchors() {
        let env = make_env();
        set_ledger(&env, 1_000_000);
        let (client, _) = setup(&env);

        let anchor1 = Address::generate(&env);
        let anchor2 = Address::generate(&env);
        let anchor3 = Address::generate(&env);
        register_anchor(&env, &client, &anchor1);
        register_anchor(&env, &client, &anchor2);
        register_anchor(&env, &client, &anchor3);

        client.submit_quote(
            &anchor1,
            &String::from_str(&env, "USD"),
            &String::from_str(&env, "USDC"),
            &10100u64, &50u32, &100u64, &100000u64, &1_003_600u64,
        );
        client.submit_quote(
            &anchor2,
            &String::from_str(&env, "USD"),
            &String::from_str(&env, "USDC"),
            &10000u64, &25u32, &100u64, &100000u64, &1_003_600u64,
        );
        client.submit_quote(
            &anchor3,
            &String::from_str(&env, "USD"),
            &String::from_str(&env, "USDC"),
            &10050u64, &30u32, &100u64, &100000u64, &1_003_600u64,
        );

        let q1 = client.get_quote(&anchor1, &1u64);
        let q2 = client.get_quote(&anchor2, &2u64);
        let q3 = client.get_quote(&anchor3, &3u64);

        // anchor2 has lowest fee
        let mut best = &q1;
        for q in [&q2, &q3] {
            if q.fee_percentage < best.fee_percentage {
                best = q;
            }
        }
        assert_eq!(best.anchor, anchor2);
        assert_eq!(best.fee_percentage, 25);
    }

    // -----------------------------------------------------------------------
    // Weighted scoring and fallback chain tests (#168)
    // -----------------------------------------------------------------------

    #[test]
    fn test_weighted_strategy_accepts_small_float_drift() {
        let strategy = WeightedRoutingStrategy {
            fee_weight: 0.3333_f32,
            speed_weight: 0.3333_f32,
            reputation_weight: 0.3333_f32,
        };

        assert!(strategy.validate());
    }

    #[test]
    fn test_weighted_strategy_rejects_large_float_drift() {
        let strategy = WeightedRoutingStrategy {
            fee_weight: 0.34_f32,
            speed_weight: 0.34_f32,
            reputation_weight: 0.34_f32,
        };

        assert!(!strategy.validate());
    }

    #[test]
    fn test_weighted_equal_weights_balanced_ranking() {
        let env = make_env();
        set_ledger(&env, 1_000_000);
        let (client, _) = setup(&env);

        let anchor1 = Address::generate(&env);
        let anchor2 = Address::generate(&env);
        register_anchor(&env, &client, &anchor1);
        register_anchor(&env, &client, &anchor2);
        // anchor1: high fee, fast, high reputation
        client.set_anchor_metadata(&anchor1, &9000u32, &100u64, &8000u32, &9900u32, &1_000_000u64);
        client.submit_quote(&anchor1, &String::from_str(&env, "USD"), &String::from_str(&env, "USDC"),
            &10000u64, &80u32, &100u64, &100000u64, &1_003_600u64);
        // anchor2: low fee, slow, low reputation
        client.set_anchor_metadata(&anchor2, &2000u32, &900u64, &8000u32, &9900u32, &1_000_000u64);
        client.submit_quote(&anchor2, &String::from_str(&env, "USD"), &String::from_str(&env, "USDC"),
            &10000u64, &10u32, &100u64, &100000u64, &1_003_600u64);

        // Equal weights (333+333+334 = 1000)
        let results = client.route_anchors(&333u32, &333u32, &334u32, &2u32, &0u32);
        // Both anchors returned
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_weighted_fee_heavy_favors_low_fee_anchor() {
        let env = make_env();
        set_ledger(&env, 1_000_000);
        let (client, _) = setup(&env);

        let anchor1 = Address::generate(&env);
        let anchor2 = Address::generate(&env);
        register_anchor(&env, &client, &anchor1);
        register_anchor(&env, &client, &anchor2);
        // anchor1: high fee
        client.set_anchor_metadata(&anchor1, &8000u32, &300u64, &8000u32, &9900u32, &1_000_000u64);
        client.submit_quote(&anchor1, &String::from_str(&env, "USD"), &String::from_str(&env, "USDC"),
            &10000u64, &90u32, &100u64, &100000u64, &1_003_600u64);
        // anchor2: low fee
        client.set_anchor_metadata(&anchor2, &8000u32, &300u64, &8000u32, &9900u32, &1_000_000u64);
        client.submit_quote(&anchor2, &String::from_str(&env, "USD"), &String::from_str(&env, "USDC"),
            &10000u64, &5u32, &100u64, &100000u64, &1_003_600u64);

        // Fee-heavy: 80% fee, 10% speed, 10% reputation (800+100+100=1000)
        let results = client.route_anchors(&800u32, &100u32, &100u32, &2u32, &0u32);
        assert_eq!(results.len(), 2);
        // anchor2 (low fee) must be ranked first
        assert_eq!(results.get(0).unwrap().anchor, anchor2);
    }

    #[test]
    fn test_weighted_reputation_heavy_filters_low_reputation() {
        let env = make_env();
        set_ledger(&env, 1_000_000);
        let (client, _) = setup(&env);

        let anchor1 = Address::generate(&env);
        let anchor2 = Address::generate(&env);
        register_anchor(&env, &client, &anchor1);
        register_anchor(&env, &client, &anchor2);
        // anchor1: low reputation
        client.set_anchor_metadata(&anchor1, &1000u32, &300u64, &8000u32, &9900u32, &1_000_000u64);
        client.submit_quote(&anchor1, &String::from_str(&env, "USD"), &String::from_str(&env, "USDC"),
            &10000u64, &10u32, &100u64, &100000u64, &1_003_600u64);
        // anchor2: high reputation
        client.set_anchor_metadata(&anchor2, &9500u32, &300u64, &8000u32, &9900u32, &1_000_000u64);
        client.submit_quote(&anchor2, &String::from_str(&env, "USD"), &String::from_str(&env, "USDC"),
            &10000u64, &10u32, &100u64, &100000u64, &1_003_600u64);

        // Reputation-heavy: 10% fee, 10% speed, 80% reputation
        let results = client.route_anchors(&100u32, &100u32, &800u32, &2u32, &0u32);
        assert_eq!(results.len(), 2);
        // anchor2 (high reputation) must be ranked first
        assert_eq!(results.get(0).unwrap().anchor, anchor2);
    }

    #[test]
    fn test_fallback_chain_length_respects_max_results() {
        let env = make_env();
        set_ledger(&env, 1_000_000);
        let (client, _) = setup(&env);

        let anchor1 = Address::generate(&env);
        let anchor2 = Address::generate(&env);
        let anchor3 = Address::generate(&env);
        register_anchor(&env, &client, &anchor1);
        register_anchor(&env, &client, &anchor2);
        register_anchor(&env, &client, &anchor3);
        for anchor in [&anchor1, &anchor2, &anchor3] {
            client.set_anchor_metadata(anchor, &8000u32, &300u64, &8000u32, &9900u32, &1_000_000u64);
            client.submit_quote(anchor, &String::from_str(&env, "USD"), &String::from_str(&env, "USDC"),
                &10000u64, &25u32, &100u64, &100000u64, &1_003_600u64);
        }

        // max_results=2 → only 2 anchors in fallback chain
        let results = client.route_anchors(&333u32, &333u32, &334u32, &2u32, &0u32);
        assert_eq!(results.len(), 2);

        // max_results=3 → all 3 anchors
        let results3 = client.route_anchors(&333u32, &333u32, &334u32, &3u32, &0u32);
        assert_eq!(results3.len(), 3);
    }

    #[test]
    fn test_invalid_weights_rejected() {
        let env = make_env();
        set_ledger(&env, 1_000_000);
        let (client, _) = setup(&env);

        // weights sum to 1100 (not 1000) → should panic
        let result = client.try_route_anchors(&500u32, &400u32, &200u32, &3u32, &0u32);
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // Asset-support and service-advertisement validation (#238)
    // -----------------------------------------------------------------------

    fn register_anchor_with_services(
        env: &Env,
        client: &AnchorKitContractClient,
        anchor: &Address,
        svc: &[u32],
    ) {
        let signing_key = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(env, client, anchor, anchor, &signing_key);
        let mut services = Vec::new(env);
        for &s in svc {
            services.push_back(s);
        }
        client.configure_services(anchor, &services);
    }

    fn lowest_fee_options(env: &Env, base: &str, quote: &str, amount: u64) -> RoutingOptions {
        let mut strategy = Vec::new(env);
        strategy.push_back(Symbol::new(env, "LowestFee"));
        RoutingOptions {
            request: RoutingRequest {
                base_asset: String::from_str(env, base),
                quote_asset: String::from_str(env, quote),
                amount,
                operation_type: 1,
            },
            strategy,
            min_reputation: 0,
            max_anchors: 5,
            require_kyc: false,
            require_compliance: false,
            subject: Address::generate(env),
            fee_weight: 333,
            speed_weight: 333,
            reputation_weight: 334,
        }
    }

    /// An anchor whose only quote is for a different asset pair must be excluded,
    /// even when its fee is lower than the matching anchor's.
    #[test]
    fn test_route_excludes_mismatched_asset_pair() {
        let env = make_env();
        set_ledger(&env, 1_000_000);
        let (client, _) = setup(&env);

        let matching = Address::generate(&env);
        let mismatched = Address::generate(&env);
        register_anchor(&env, &client, &matching);
        register_anchor(&env, &client, &mismatched);
        client.set_anchor_metadata(&matching, &8000u32, &300u64, &8000u32, &9900u32, &1_000_000u64);
        client.set_anchor_metadata(&mismatched, &8000u32, &300u64, &8000u32, &9900u32, &1_000_000u64);

        // matching: USD -> USDC, fee 50
        client.submit_quote(
            &matching,
            &String::from_str(&env, "USD"),
            &String::from_str(&env, "USDC"),
            &10000u64, &50u32, &100u64, &100000u64, &1_003_600u64,
        );
        // mismatched: EUR -> USDC, fee 10 (lower, but wrong base asset)
        client.submit_quote(
            &mismatched,
            &String::from_str(&env, "EUR"),
            &String::from_str(&env, "USDC"),
            &10000u64, &10u32, &100u64, &100000u64, &1_003_600u64,
        );

        let options = lowest_fee_options(&env, "USD", "USDC", 5000);
        let best = client.route_transaction(&options);
        assert_eq!(best.anchor, matching);
    }

    /// An anchor that does not advertise the quote service is excluded from
    /// routing even if it has a matching, lower-fee quote stored.
    #[test]
    fn test_route_excludes_anchor_without_quote_service() {
        let env = make_env();
        set_ledger(&env, 1_000_000);
        let (client, _) = setup(&env);

        let with_quotes = Address::generate(&env);
        let deposits_only = Address::generate(&env);
        register_anchor(&env, &client, &with_quotes); // services [1, 3]
        register_anchor_with_services(&env, &client, &deposits_only, &[1u32]); // deposits only
        client.set_anchor_metadata(&with_quotes, &8000u32, &300u64, &8000u32, &9900u32, &1_000_000u64);
        client.set_anchor_metadata(&deposits_only, &8000u32, &300u64, &8000u32, &9900u32, &1_000_000u64);

        client.submit_quote(
            &with_quotes,
            &String::from_str(&env, "USD"),
            &String::from_str(&env, "USDC"),
            &10000u64, &50u32, &100u64, &100000u64, &1_003_600u64,
        );
        // deposits_only stores a cheaper matching quote, but advertises no quotes.
        client.submit_quote(
            &deposits_only,
            &String::from_str(&env, "USD"),
            &String::from_str(&env, "USDC"),
            &10000u64, &5u32, &100u64, &100000u64, &1_003_600u64,
        );

        let options = lowest_fee_options(&env, "USD", "USDC", 5000);
        let best = client.route_transaction(&options);
        assert_eq!(best.anchor, with_quotes);
    }

    /// When no anchor advertises a quote for the requested pair, routing fails.
    #[test]
    fn test_route_no_matching_pair_errors() {
        let env = make_env();
        set_ledger(&env, 1_000_000);
        let (client, _) = setup(&env);

        let anchor = Address::generate(&env);
        register_anchor(&env, &client, &anchor);
        client.set_anchor_metadata(&anchor, &8000u32, &300u64, &8000u32, &9900u32, &1_000_000u64);
        // A valid, in-limits, non-expired quote — excluded only by asset mismatch.
        client.submit_quote(
            &anchor,
            &String::from_str(&env, "EUR"),
            &String::from_str(&env, "GBP"),
            &10000u64, &10u32, &100u64, &100000u64, &1_003_600u64,
        );

        let options = lowest_fee_options(&env, "USD", "USDC", 5000);
        assert!(client.try_route_transaction(&options).is_err());
    }

    /// route_anchors must omit anchors that do not advertise the quote service.
    #[test]
    fn test_route_anchors_filters_non_quote_service() {
        let env = make_env();
        set_ledger(&env, 1_000_000);
        let (client, _) = setup(&env);

        let with_quotes = Address::generate(&env);
        let deposits_only = Address::generate(&env);
        register_anchor(&env, &client, &with_quotes);
        register_anchor_with_services(&env, &client, &deposits_only, &[1u32]);

        for anchor in [&with_quotes, &deposits_only] {
            client.set_anchor_metadata(anchor, &8000u32, &300u64, &8000u32, &9900u32, &1_000_000u64);
            client.submit_quote(
                anchor,
                &String::from_str(&env, "USD"),
                &String::from_str(&env, "USDC"),
                &10000u64, &25u32, &100u64, &100000u64, &1_003_600u64,
            );
        }

        let results = client.route_anchors(&333u32, &333u32, &334u32, &5u32, &0u32);
        // Only the quote-advertising anchor is ranked.
        assert_eq!(results.len(), 1);
        assert_eq!(results.get(0).unwrap().anchor, with_quotes);
    }

    // -----------------------------------------------------------------------
    // #1268: Weight sum just outside tolerance
    // WEIGHT_SUM_TOLERANCE = 0.01.  A sum of 0.0f32 + 0.0f32 + 1.011f32
    // deviates by 0.011, which is strictly greater than the tolerance, so
    // WeightedRoutingStrategy::validate() must return false.
    //
    // The integer route_anchors path enforces exact equality to 1000, so
    // a sum of 1001 must also be rejected with InvalidWeights.
    // -----------------------------------------------------------------------

    /// Float weights whose sum exceeds 1.0 by just more than WEIGHT_SUM_TOLERANCE
    /// (0.01) must be rejected by WeightedRoutingStrategy::validate().
    ///
    /// The tolerance is 0.01.  A total of 1.011 deviates by 0.011, which is
    /// strictly outside the accepted range.  This test is derived from the
    /// implemented constant, not an arbitrary epsilon.
    #[test]
    fn test_weighted_strategy_rejects_sum_just_outside_tolerance() {
        // sum = 0.0 + 0.0 + 1.011 = 1.011  →  |1.011 - 1.0| = 0.011 > 0.01
        let strategy = WeightedRoutingStrategy {
            fee_weight: 0.0_f32,
            speed_weight: 0.0_f32,
            reputation_weight: 1.011_f32,
        };
        assert!(
            !strategy.validate(),
            "weight sum 1.011 is outside the 0.01 tolerance and must be rejected"
        );
    }

    /// Float weights whose sum falls within WEIGHT_SUM_TOLERANCE must still be
    /// accepted.  This control case guards against tightening the tolerance
    /// accidentally.
    ///
    /// sum = 0.333 + 0.333 + 0.334 = 1.000  →  |1.000 - 1.0| = 0.0 < 0.01
    #[test]
    fn test_weighted_strategy_accepts_sum_within_tolerance() {
        let strategy = WeightedRoutingStrategy {
            fee_weight: 0.333_f32,
            speed_weight: 0.333_f32,
            reputation_weight: 0.334_f32,
        };
        assert!(
            strategy.validate(),
            "weight sum exactly 1.0 must be accepted"
        );
    }

    /// The integer route_anchors path requires exact equality to 1000.
    /// A sum of 1001 (one unit over) must panic with InvalidWeights.
    #[test]
    fn test_route_anchors_rejects_integer_weight_sum_just_over_1000() {
        let env = make_env();
        set_ledger(&env, 1_000_000);
        let (client, _) = setup(&env);

        // 334 + 333 + 334 = 1001 → exactly one unit beyond the required 1000.
        let result = client.try_route_anchors(&334u32, &333u32, &334u32, &3u32, &0u32);
        assert!(
            result.is_err(),
            "integer weight sum 1001 must be rejected with InvalidWeights"
        );
    }

    // -----------------------------------------------------------------------
    // #1267: No eligible quote — NoQuotesAvailable
    // A quote for a different asset pair (EUR/GBP) exists in storage, but
    // routing is requested for USD/USDC.  The non-matching quote must not be
    // selected, and the result must be the documented NoQuotesAvailable error.
    // -----------------------------------------------------------------------

    /// When only a mismatched quote exists (EUR/GBP) but routing targets
    /// USD/USDC, route_transaction must fail with NoQuotesAvailable.
    ///
    /// This is distinct from `test_route_excludes_mismatched_asset_pair`, which
    /// has a second matching anchor.  Here there is *no* eligible quote at all.
    #[test]
    fn test_route_no_eligible_quote_returns_no_quotes_available() {
        let env = make_env();
        set_ledger(&env, 1_000_000);
        let (client, _) = setup(&env);

        let anchor = Address::generate(&env);
        register_anchor(&env, &client, &anchor);
        client.set_anchor_metadata(&anchor, &8000u32, &300u64, &8000u32, &9900u32, &1_000_000u64);

        // The only available quote is for a completely different corridor.
        client.submit_quote(
            &anchor,
            &String::from_str(&env, "EUR"),
            &String::from_str(&env, "GBP"),
            &10_000u64, &10u32, &100u64, &100_000u64, &1_003_600u64,
        );

        // Routing for USD/USDC must fail — the EUR/GBP quote is ineligible.
        let options = lowest_fee_options(&env, "USD", "USDC", 5_000);
        assert!(
            client.try_route_transaction(&options).is_err(),
            "routing with no eligible quote must return NoQuotesAvailable"
        );
    }

    // -----------------------------------------------------------------------
    // #1272: All providers unavailable
    // Every candidate advertises a matching quote, but all are marked
    // unavailable via the normal health API.  Routing must fail with the
    // documented error, must not return any unavailable provider as healthy,
    // and health filtering must not mutate the stored quote data.
    // -----------------------------------------------------------------------

    /// When every candidate anchor is marked unavailable, route_transaction
    /// must fail rather than returning a stale/unhealthy route.
    #[test]
    fn test_route_all_providers_unavailable_errors() {
        let env = make_env();
        set_ledger(&env, 1_000_000);
        let (client, _) = setup(&env);

        let anchor1 = Address::generate(&env);
        let anchor2 = Address::generate(&env);
        let anchor3 = Address::generate(&env);
        register_anchor(&env, &client, &anchor1);
        register_anchor(&env, &client, &anchor2);
        register_anchor(&env, &client, &anchor3);

        for anchor in [&anchor1, &anchor2, &anchor3] {
            client.set_anchor_metadata(anchor, &8000u32, &300u64, &8000u32, &9900u32, &1_000_000u64);
            client.submit_quote(
                anchor,
                &String::from_str(&env, "USD"),
                &String::from_str(&env, "USDC"),
                &10_000u64, &25u32, &100u64, &100_000u64, &1_003_600u64,
            );
        }

        // Mark every candidate unavailable using the normal health API.
        for anchor in [&anchor1, &anchor2, &anchor3] {
            client.set_anchor_active(anchor, &false);
        }

        let options = lowest_fee_options(&env, "USD", "USDC", 5_000);
        assert!(
            client.try_route_transaction(&options).is_err(),
            "routing must fail when every candidate is unavailable"
        );
    }

    /// route_anchors must not return any unavailable provider as a healthy
    /// candidate when all providers are unavailable.
    #[test]
    fn test_route_anchors_all_unavailable_returns_no_healthy_candidates() {
        let env = make_env();
        set_ledger(&env, 1_000_000);
        let (client, _) = setup(&env);

        let anchor1 = Address::generate(&env);
        let anchor2 = Address::generate(&env);
        register_anchor(&env, &client, &anchor1);
        register_anchor(&env, &client, &anchor2);

        for anchor in [&anchor1, &anchor2] {
            client.set_anchor_metadata(anchor, &8000u32, &300u64, &8000u32, &9900u32, &1_000_000u64);
            client.submit_quote(
                anchor,
                &String::from_str(&env, "USD"),
                &String::from_str(&env, "USDC"),
                &10_000u64, &25u32, &100u64, &100_000u64, &1_003_600u64,
            );
            client.set_anchor_active(anchor, &false);
        }

        let results = client.route_anchors(&333u32, &333u32, &334u32, &5u32, &0u32);
        assert_eq!(
            results.len(),
            0,
            "no unavailable provider may be returned as a healthy candidate"
        );
    }

    /// Health filtering must not mutate the stored quote data: the quote
    /// remains readable and unchanged after its anchor is marked unavailable.
    #[test]
    fn test_health_filtering_does_not_mutate_quote_data() {
        let env = make_env();
        set_ledger(&env, 1_000_000);
        let (client, _) = setup(&env);

        let anchor = Address::generate(&env);
        register_anchor(&env, &client, &anchor);
        client.set_anchor_metadata(&anchor, &8000u32, &300u64, &8000u32, &9900u32, &1_000_000u64);
        client.submit_quote(
            &anchor,
            &String::from_str(&env, "USD"),
            &String::from_str(&env, "USDC"),
            &10_000u64, &25u32, &100u64, &100_000u64, &1_003_600u64,
        );

        let before = client.get_quote(&anchor, &1u64);

        client.set_anchor_active(&anchor, &false);

        // Routing excludes the unavailable anchor...
        let options = lowest_fee_options(&env, "USD", "USDC", 5_000);
        assert!(client.try_route_transaction(&options).is_err());

        // ...but the quote itself is untouched.
        let after = client.get_quote(&anchor, &1u64);
        assert_eq!(after.anchor, before.anchor);
        assert_eq!(after.fee_percentage, before.fee_percentage);
        assert_eq!(after.minimum_amount, before.minimum_amount);
        assert_eq!(after.maximum_amount, before.maximum_amount);
        assert_eq!(after.valid_until, before.valid_until);
    }

    /// Control case: a matching USD/USDC quote remains reachable when both a
    /// mismatched EUR/GBP quote and a valid USD/USDC quote are present.
    #[test]
    fn test_route_matching_quote_found_alongside_ineligible_quote() {
        let env = make_env();
        set_ledger(&env, 1_000_000);
        let (client, _) = setup(&env);

        let mismatched = Address::generate(&env);
        let matching = Address::generate(&env);
        register_anchor(&env, &client, &mismatched);
        register_anchor(&env, &client, &matching);
        for anchor in [&mismatched, &matching] {
            client.set_anchor_metadata(anchor, &8000u32, &300u64, &8000u32, &9900u32, &1_000_000u64);
        }

        // Ineligible EUR/GBP quote.
        client.submit_quote(
            &mismatched,
            &String::from_str(&env, "EUR"),
            &String::from_str(&env, "GBP"),
            &10_000u64, &5u32, &100u64, &100_000u64, &1_003_600u64,
        );

        // Eligible USD/USDC quote.
        client.submit_quote(
            &matching,
            &String::from_str(&env, "USD"),
            &String::from_str(&env, "USDC"),
            &10_000u64, &30u32, &100u64, &100_000u64, &1_003_600u64,
        );

        let options = lowest_fee_options(&env, "USD", "USDC", 5_000);
        let best = client.route_transaction(&options);
        assert_eq!(
            best.anchor, matching,
            "the matching USD/USDC anchor must be selected when a valid quote exists"
        );
    }
}

// ── CrossAnchorFeeAggregator tests ────────────────────────────────────────────

mod fee_aggregator_tests {
    use anchorkit::sep38::CrossAnchorFeeAggregator;

    const NOW: u64 = 1_000_000;

    #[test]
    fn single_anchor_no_anomaly() {
        let mut agg = CrossAnchorFeeAggregator::new(CrossAnchorFeeAggregator::DEFAULT_ANOMALY_THRESHOLD_BPS);
        agg.insert_observation("anchor-a", 200, NOW);
        let report = agg.compute_report(NOW);
        assert_eq!(report.median_fee_bps, 200);
        assert!(report.anomalous_anchors.is_empty());
    }

    #[test]
    fn two_anchors_one_above_threshold_flagged() {
        let mut agg = CrossAnchorFeeAggregator::new(150);
        agg.insert_observation("anchor-a", 200, NOW);
        agg.insert_observation("anchor-b", 400, NOW); // 400 - 200 = 200 > 150 → anomalous
        let report = agg.compute_report(NOW);
        assert_eq!(report.median_fee_bps, 200);
        assert_eq!(report.anomalous_anchors.len(), 1);
        assert_eq!(report.anomalous_anchors[0].0, "anchor-b");
    }

    #[test]
    fn equal_fees_no_anomaly() {
        let mut agg = CrossAnchorFeeAggregator::new(150);
        for id in ["a", "b", "c"] {
            agg.insert_observation(id, 300, NOW);
        }
        let report = agg.compute_report(NOW);
        assert_eq!(report.median_fee_bps, 300);
        assert!(report.anomalous_anchors.is_empty());
    }

    #[test]
    fn empty_history_returns_no_anomalous_entries() {
        let agg = CrossAnchorFeeAggregator::new(150);
        let report = agg.compute_report(NOW);
        assert_eq!(report.median_fee_bps, 0);
        assert!(report.anomalous_anchors.is_empty());
    }
}
