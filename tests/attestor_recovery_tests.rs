#![cfg(test)]

mod sep10_test_util;

mod attestor_recovery_tests {
    use soroban_sdk::{
        testutils::{Address as _, Ledger, LedgerInfo},
        Address, Bytes, Env, Error,
    };
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    use anchorkit::contract::{AnchorKitContract, AnchorKitContractClient};
    use anchorkit::errors::ErrorCode;
    use crate::sep10_test_util::{register_attestor_with_sep10, sign_payload};

    fn make_env() -> Env {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set(LedgerInfo {
            timestamp: 1_000_000,
            protocol_version: 21,
            sequence_number: 0,
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });
        env
    }

    fn payload(env: &Env) -> Bytes {
        let mut b = Bytes::new(env);
        for i in 0u8..32 {
            b.push_back(i);
        }
        b
    }

    fn setup(env: &Env) -> (AnchorKitContractClient, Address, Address, SigningKey) {
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(env, &contract_id);
        let admin = Address::generate(env);
        let issuer = Address::generate(env);
        client.initialize(&admin);
        let sk = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(env, &client, &issuer, &issuer, &sk);
        (client, admin, issuer, sk)
    }

    // -----------------------------------------------------------------------
    // Revoke then verify the attestor is no longer active
    // -----------------------------------------------------------------------

    #[test]
    fn test_revoke_makes_attestor_inactive() {
        let env = make_env();
        let (client, _, issuer, _) = setup(&env);

        assert!(client.is_attestor(&issuer), "attestor should be active after registration");

        client.revoke_attestor(&issuer);

        assert!(!client.is_attestor(&issuer), "attestor must be inactive after revocation");
    }

    // -----------------------------------------------------------------------
    // Revocation record is preserved after revoking
    // -----------------------------------------------------------------------

    #[test]
    fn test_revocation_record_is_stored() {
        let env = make_env();
        let (client, _, issuer, _) = setup(&env);

        client.revoke_attestor(&issuer);

        let record_opt = client.get_attestor_revocation_info(&issuer);
        assert!(record_opt.is_some(), "revocation record must exist after revoking");

        let record = record_opt.unwrap();
        assert!(!record.reactivated, "record.reactivated must be false before recovery");
        assert_eq!(record.reactivated_at, 0, "reactivated_at must be 0 before recovery");
    }

    // -----------------------------------------------------------------------
    // Attestor can be reactivated and becomes active again
    // -----------------------------------------------------------------------

    #[test]
    fn test_reactivate_restores_active_status() {
        let env = make_env();
        let (client, _, issuer, _) = setup(&env);

        client.revoke_attestor(&issuer);
        assert!(!client.is_attestor(&issuer));

        client.reactivate_attestor(&issuer);

        assert!(client.is_attestor(&issuer), "attestor must be active after reactivation");
    }

    // -----------------------------------------------------------------------
    // Reactivated attestor can submit attestations again
    // -----------------------------------------------------------------------

    #[test]
    fn test_reactivated_attestor_can_submit_attestations() {
        let env = make_env();
        let (client, _, issuer, sk) = setup(&env);
        let subject = Address::generate(&env);

        client.revoke_attestor(&issuer);
        client.reactivate_attestor(&issuer);

        let hash = payload(&env);
        let sig = sign_payload(&env, &sk, &hash);
        // Must not panic — the reactivated attestor should be accepted.
        let id = client.submit_attestation(&issuer, &subject, &1_000_001u64, &hash, &sig);
        assert_eq!(id, 0, "first attestation must have id 0");
    }

    // -----------------------------------------------------------------------
    // Revocation record is marked reactivated after recovery
    // -----------------------------------------------------------------------

    #[test]
    fn test_revocation_record_updated_after_reactivation() {
        let env = make_env();
        let (client, _, issuer, _) = setup(&env);

        client.revoke_attestor(&issuer);

        // Advance ledger time so reactivated_at is distinguishable.
        env.ledger().set(LedgerInfo {
            timestamp: 2_000_000,
            protocol_version: 21,
            sequence_number: 0,
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });

        client.reactivate_attestor(&issuer);

        let record = client.get_attestor_revocation_info(&issuer).expect("record must exist");
        assert!(record.reactivated, "record.reactivated must be true after recovery");
        assert_eq!(record.reactivated_at, 2_000_000, "reactivated_at must equal ledger timestamp");
    }

    // -----------------------------------------------------------------------
    // Revocation audit data is preserved after reactivation
    // -----------------------------------------------------------------------

    #[test]
    fn test_revocation_audit_data_preserved_after_reactivation() {
        let env = make_env();
        let (client, _, issuer, _) = setup(&env);

        client.revoke_attestor(&issuer);
        let record_before = client.get_attestor_revocation_info(&issuer).unwrap();
        let original_revoked_at = record_before.revoked_at;

        client.reactivate_attestor(&issuer);

        let record_after = client.get_attestor_revocation_info(&issuer).unwrap();
        assert_eq!(
            record_after.revoked_at, original_revoked_at,
            "revoked_at timestamp must be preserved after reactivation"
        );
    }

    // -----------------------------------------------------------------------
    // Reactivating a non-revoked (still active) attestor fails
    // -----------------------------------------------------------------------

    #[test]
    #[should_panic]
    fn test_reactivate_active_attestor_panics() {
        let env = make_env();
        let (client, _, issuer, _) = setup(&env);
        // Attestor is still active — reactivation must fail.
        client.reactivate_attestor(&issuer);
    }

    // -----------------------------------------------------------------------
    // Reactivating an attestor with no prior revocation record fails
    // -----------------------------------------------------------------------

    #[test]
    #[should_panic]
    fn test_reactivate_unregistered_attestor_panics() {
        let env = make_env();
        let (client, _, _, _) = setup(&env);
        let stranger = Address::generate(&env);
        // This address was never registered — no revocation record exists.
        client.reactivate_attestor(&stranger);
    }

    // -----------------------------------------------------------------------
    // Revoking an already-revoked attestor (not registered) fails
    // -----------------------------------------------------------------------

    #[test]
    #[should_panic]
    fn test_revoke_already_revoked_attestor_panics() {
        let env = make_env();
        let (client, _, issuer, _) = setup(&env);
        client.revoke_attestor(&issuer);
        // Second revocation must fail with AttestorNotRegistered.
        client.revoke_attestor(&issuer);
    }

    // -----------------------------------------------------------------------
    // #1254 — Repeated revocation is explicitly rejected, not idempotent
    // -----------------------------------------------------------------------

    #[test]
    fn test_revoke_twice_returns_attestor_not_registered() {
        let env = make_env();
        let (client, _, issuer, _) = setup(&env);

        client.revoke_attestor(&issuer);
        let count_after_first = client.get_attestor_count();
        let record_after_first = client
            .get_attestor_revocation_info(&issuer)
            .expect("first revocation must store a revocation record");

        // Operator automation that retries a revoke receives a specific
        // AttestorNotRegistered error rather than a silent success.
        let err = client
            .try_revoke_attestor(&issuer)
            .expect_err("second revocation must be rejected")
            .expect("rejection must be a contract error, not a host failure");
        assert_eq!(err, Error::from_contract_error(ErrorCode::AttestorNotRegistered as u32));

        // The rejected call leaves the revoked state exactly as it was.
        assert!(!client.is_attestor(&issuer));
        assert_eq!(client.get_attestor_count(), count_after_first);
        let record = client
            .get_attestor_revocation_info(&issuer)
            .expect("revocation record must survive the rejected call");
        assert_eq!(record.revoked_at, record_after_first.revoked_at);
        assert_eq!(record.revoked_by, record_after_first.revoked_by);
        assert!(!record.reactivated);
    }

    // -----------------------------------------------------------------------
    // get_attestor_revocation_info returns None for never-revoked attestors
    // -----------------------------------------------------------------------

    #[test]
    fn test_no_revocation_info_for_active_attestor() {
        let env = make_env();
        let (client, _, issuer, _) = setup(&env);

        // Still active — no revocation record should exist.
        let info = client.get_attestor_revocation_info(&issuer);
        assert!(info.is_none(), "no revocation record for an active attestor");
    }

    // -----------------------------------------------------------------------
    // Full revoke-reactivate-revoke cycle preserves consistency
    // -----------------------------------------------------------------------

    #[test]
    fn test_revoke_reactivate_revoke_cycle() {
        let env = make_env();
        let (client, _, issuer, _) = setup(&env);

        // First revocation
        client.revoke_attestor(&issuer);
        assert!(!client.is_attestor(&issuer));

        // Recovery
        client.reactivate_attestor(&issuer);
        assert!(client.is_attestor(&issuer));

        // Second revocation
        client.revoke_attestor(&issuer);
        assert!(!client.is_attestor(&issuer));
    }
}
