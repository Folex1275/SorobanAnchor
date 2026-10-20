/// Tests for issue #657: Multi-anchor reputation weighting.
///
/// Covers:
/// - Setting and retrieving per-anchor reputation records.
/// - Getting and setting contract-wide reputation weights.
/// - Computing composite reputation scores.
/// - Ranking anchors deterministically (tie-breaking by anchor XDR order).
/// - Validation: successful_routed > total_routed is rejected.
/// - Validation: invalid reputation weights are rejected.
/// - New anchors with no record return score 0.
#[cfg(test)]

#[path = "sep10_test_util.rs"]
mod sep10_test_util;

mod reputation_weighting_tests {
    use soroban_sdk::{
        testutils::{Address as _, Ledger, LedgerInfo},
        Address, Env,
    };

    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    use anchorkit::contract::{
        AnchorKitContract, AnchorKitContractClient, ReputationWeights,
    };
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
        let mut services = soroban_sdk::Vec::new(env);
        services.push_back(1u32);
        services.push_back(3u32);
        client.configure_services(anchor, &services);
        client.set_anchor_metadata(anchor, &8000u32, &300u64, &8000u32, &9900u32, &1_000_000u64);
    }

    // -----------------------------------------------------------------------
    // Reputation record CRUD
    // -----------------------------------------------------------------------

    #[test]
    fn test_set_and_get_anchor_reputation() {
        let env = make_env();
        set_ledger(&env, 1_000_000);
        let (client, _) = setup(&env);
        let anchor = Address::generate(&env);
        register_anchor(&env, &client, &anchor);

        client.set_anchor_reputation(
            &anchor,
            &100u64, // total_routed
            &95u64,  // successful_routed
            &8000u32, // operator_quality_score
            &9900u64, // uptime_ticks
            &10000u64, // total_ticks
        );

        let record = client.get_anchor_reputation(&anchor);
        assert!(record.is_some());
        let r = record.unwrap();
        assert_eq!(r.total_routed, 100);
        assert_eq!(r.successful_routed, 95);
        assert_eq!(r.operator_quality_score, 8000);
        assert_eq!(r.uptime_ticks, 9900);
        assert_eq!(r.total_ticks, 10000);
    }

    #[test]
    fn test_get_anchor_reputation_returns_none_when_unset() {
        let env = make_env();
        set_ledger(&env, 1_000_000);
        let (client, _) = setup(&env);
        let anchor = Address::generate(&env);
        register_anchor(&env, &client, &anchor);

        let record = client.get_anchor_reputation(&anchor);
        assert!(record.is_none());
    }

    #[test]
    #[should_panic]
    fn test_set_reputation_rejects_successful_greater_than_total() {
        let env = make_env();
        set_ledger(&env, 1_000_000);
        let (client, _) = setup(&env);
        let anchor = Address::generate(&env);
        register_anchor(&env, &client, &anchor);

        // successful_routed (120) > total_routed (100) → should panic
        client.set_anchor_reputation(
            &anchor,
            &100u64,
            &120u64, // invalid
            &8000u32,
            &9000u64,
            &10000u64,
        );
    }

    #[test]
    #[should_panic]
    fn test_set_reputation_rejects_operator_quality_above_10000() {
        let env = make_env();
        set_ledger(&env, 1_000_000);
        let (client, _) = setup(&env);
        let anchor = Address::generate(&env);
        register_anchor(&env, &client, &anchor);

        // operator_quality_score > 10 000 → should panic
        client.set_anchor_reputation(
            &anchor,
            &100u64,
            &90u64,
            &10_001u32, // invalid
            &9000u64,
            &10000u64,
        );
    }

    // -----------------------------------------------------------------------
    // Reputation weights
    // -----------------------------------------------------------------------

    #[test]
    fn test_default_reputation_weights_are_valid() {
        let weights = ReputationWeights::default_weights();
        assert!(weights.is_valid());
        assert_eq!(
            weights.success_rate_weight + weights.uptime_weight + weights.operator_quality_weight,
            1_000
        );
    }

    #[test]
    fn test_set_and_get_reputation_weights() {
        let env = make_env();
        set_ledger(&env, 1_000_000);
        let (client, _) = setup(&env);

        client.set_reputation_weights(&500u32, &300u32, &200u32);
        let w = client.get_reputation_weights();
        assert_eq!(w.success_rate_weight, 500);
        assert_eq!(w.uptime_weight, 300);
        assert_eq!(w.operator_quality_weight, 200);
    }

    #[test]
    #[should_panic]
    fn test_invalid_reputation_weights_rejected() {
        let env = make_env();
        set_ledger(&env, 1_000_000);
        let (client, _) = setup(&env);

        // 400 + 300 + 200 = 900 ≠ 1 000 → should panic
        client.set_reputation_weights(&400u32, &300u32, &200u32);
    }

    // -----------------------------------------------------------------------
    // Composite score
    // -----------------------------------------------------------------------

    #[test]
    fn test_composite_reputation_is_zero_for_unknown_anchor() {
        let env = make_env();
        set_ledger(&env, 1_000_000);
        let (client, _) = setup(&env);
        let anchor = Address::generate(&env);
        // Never registered — should return 0 without panicking.
        let score = client.get_anchor_composite_reputation(&anchor);
        assert_eq!(score, 0);
    }

    #[test]
    fn test_perfect_anchor_scores_near_max() {
        let env = make_env();
        set_ledger(&env, 1_000_000);
        let (client, _) = setup(&env);
        let anchor = Address::generate(&env);
        register_anchor(&env, &client, &anchor);

        client.set_anchor_reputation(
            &anchor,
            &1000u64,
            &1000u64,  // 100 % success
            &10_000u32, // max operator quality
            &10000u64,
            &10000u64, // 100 % uptime
        );

        let score = client.get_anchor_composite_reputation(&anchor);
        // Perfect on all sub-scores → should be 10 000
        assert_eq!(score, 10_000);
    }

    #[test]
    fn test_zero_activity_anchor_scores_below_perfect() {
        let env = make_env();
        set_ledger(&env, 1_000_000);
        let (client, _) = setup(&env);
        let anchor = Address::generate(&env);
        register_anchor(&env, &client, &anchor);

        client.set_anchor_reputation(
            &anchor,
            &0u64,  // no routed yet → success_rate defaults to 1.0
            &0u64,
            &0u32,  // no operator quality
            &0u64,  // no uptime ticks
            &0u64,  // no total ticks → uptime defaults to 1.0
        );

        let score = client.get_anchor_composite_reputation(&anchor);
        // success_rate=1.0 (default), uptime=1.0 (default), operator=0.0
        // With default weights (0.40, 0.35, 0.25):
        // = 0.40 * 1.0 + 0.35 * 1.0 + 0.25 * 0.0 = 0.75 → 7 500
        assert_eq!(score, 7_500);
    }

    #[test]
    fn test_high_failure_rate_lowers_score() {
        let env = make_env();
        set_ledger(&env, 1_000_000);
        let (client, _) = setup(&env);
        let anchor = Address::generate(&env);
        register_anchor(&env, &client, &anchor);

        client.set_anchor_reputation(
            &anchor,
            &100u64,
            &20u64,   // 20 % success rate
            &5000u32, // moderate operator quality
            &10000u64,
            &10000u64, // perfect uptime
        );

        let score_low_success = client.get_anchor_composite_reputation(&anchor);

        // Compare to an anchor with perfect success rate
        let anchor2 = Address::generate(&env);
        register_anchor(&env, &client, &anchor2);
        client.set_anchor_reputation(
            &anchor2,
            &100u64,
            &100u64,   // 100 % success rate
            &5000u32,
            &10000u64,
            &10000u64,
        );
        let score_high_success = client.get_anchor_composite_reputation(&anchor2);

        assert!(score_high_success > score_low_success);
    }

    // -----------------------------------------------------------------------
    // Ranking
    // -----------------------------------------------------------------------

    #[test]
    fn test_rank_anchors_by_reputation_descending() {
        let env = make_env();
        set_ledger(&env, 1_000_000);
        let (client, _) = setup(&env);

        let anchor_low  = Address::generate(&env);
        let anchor_high = Address::generate(&env);
        register_anchor(&env, &client, &anchor_low);
        register_anchor(&env, &client, &anchor_high);

        // low quality
        client.set_anchor_reputation(&anchor_low,  &100u64, &50u64,  &1000u32, &5000u64, &10000u64);
        // high quality
        client.set_anchor_reputation(&anchor_high, &100u64, &99u64, &9000u32, &9800u64, &10000u64);

        let ranked = client.rank_anchors_by_reputation();
        assert_eq!(ranked.len(), 2);
        // High-quality anchor should be ranked first.
        assert_eq!(ranked.get(0).unwrap(), anchor_high);
        assert_eq!(ranked.get(1).unwrap(), anchor_low);
    }

    #[test]
    fn test_rank_anchors_tie_is_deterministic() {
        let env = make_env();
        set_ledger(&env, 1_000_000);
        let (client, _) = setup(&env);

        let anchor_a = Address::generate(&env);
        let anchor_b = Address::generate(&env);
        register_anchor(&env, &client, &anchor_a);
        register_anchor(&env, &client, &anchor_b);

        // Both anchors get identical reputation records → tie in composite score.
        for anchor in [&anchor_a, &anchor_b] {
            client.set_anchor_reputation(
                anchor,
                &100u64, &90u64, &7000u32, &8000u64, &10000u64,
            );
        }

        // Run ranking twice and verify the order is stable.
        let ranked1 = client.rank_anchors_by_reputation();
        let ranked2 = client.rank_anchors_by_reputation();
        assert_eq!(ranked1.len(), 2);
        assert_eq!(ranked2.len(), 2);
        // Same ordering both times.
        assert_eq!(ranked1.get(0).unwrap(), ranked2.get(0).unwrap());
        assert_eq!(ranked1.get(1).unwrap(), ranked2.get(1).unwrap());
    }

    #[test]
    fn test_zero_reputation_anchor_scores_zero_and_ranks_last() {
        let env = make_env();
        set_ledger(&env, 1_000_000);
        let (client, _) = setup(&env);

        let anchor_zero = Address::generate(&env);
        let anchor_positive = Address::generate(&env);
        register_anchor(&env, &client, &anchor_zero);
        register_anchor(&env, &client, &anchor_positive);

        // Zero-reputation anchor: all sub-scores are zero.
        client.set_anchor_reputation(
            &anchor_zero,
            &0u64, &0u64, &0u32, &0u64, &0u64,
        );
        // Positive-score alternative.
        client.set_anchor_reputation(
            &anchor_positive,
            &100u64, &95u64, &9000u32, &9900u64, &10000u64,
        );

        // Zero reputation must not panic or produce invalid arithmetic.
        let zero_score = client.get_anchor_composite_reputation(&anchor_zero);
        assert_eq!(zero_score, 0);

        // Positive-score behavior is unchanged and strictly greater than zero.
        let positive_score = client.get_anchor_composite_reputation(&anchor_positive);
        assert!(positive_score > 0);
        assert!(positive_score > zero_score);

        // Ranking policy: positive-score anchor preferred, zero-score ranked last.
        let ranked = client.rank_anchors_by_reputation();
        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked.get(0).unwrap(), anchor_positive);
        assert_eq!(ranked.get(1).unwrap(), anchor_zero);
    }

    #[test]
    fn test_rank_anchors_no_record_scores_zero_and_appears_last() {
        let env = make_env();
        set_ledger(&env, 1_000_000);
        let (client, _) = setup(&env);

        let anchor_scored   = Address::generate(&env);
        let anchor_unscored = Address::generate(&env);
        register_anchor(&env, &client, &anchor_scored);
        register_anchor(&env, &client, &anchor_unscored);

        // Only one anchor has a reputation record.
        client.set_anchor_reputation(
            &anchor_scored,
            &100u64, &95u64, &9000u32, &9900u64, &10000u64,
        );
        // anchor_unscored has no record → score 0.

        let ranked = client.rank_anchors_by_reputation();
        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked.get(0).unwrap(), anchor_scored);
        assert_eq!(ranked.get(1).unwrap(), anchor_unscored);
    }
}
