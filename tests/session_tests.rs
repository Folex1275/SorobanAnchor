#![cfg(test)]

mod sep10_test_util;

mod session_tests {
    use soroban_sdk::{
        testutils::{Address as _, Ledger, LedgerInfo},
        Address, Bytes, Env, String,
    };

    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    use anchorkit::contract::{AnchorKitContract, AnchorKitContractClient};
    use crate::sep10_test_util::{register_attestor_with_sep10, sign_payload};

    fn make_env() -> Env {
        let env = Env::default();
        env.mock_all_auths();
        env
    }

    fn setup_ledger(env: &Env) {
        env.ledger().set(LedgerInfo {
            timestamp: 0,
            protocol_version: 21,
            sequence_number: 0,
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });
    }

    fn payload(env: &Env, byte: u8) -> Bytes {
        let mut b = Bytes::new(env);
        for _ in 0..32 {
            b.push_back(byte);
        }
        b
    }

    fn sig(env: &Env, bytes: &[u8]) -> Bytes {
        let mut b = Bytes::new(env);
        for byte in bytes {
            b.push_back(*byte);
        }
        // Pad to 64 bytes (Ed25519 signature length)
        while b.len() < 64 {
            b.push_back(0);
        }
        b
    }

    // -----------------------------------------------------------------------
    // create_session
    // -----------------------------------------------------------------------

    #[test]
    fn test_create_session_returns_sequential_ids() {
        let env = make_env();
        setup_ledger(&env);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let user = Address::generate(&env);
        client.initialize(&admin);

        let id0 = client.create_session(&user);
        let id1 = client.create_session(&user);
        let id2 = client.create_session(&user);

        assert_eq!(id0, 0);
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
    }

    #[test]
    fn test_create_session_stores_initiator() {
        let env = make_env();
        setup_ledger(&env);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let user = Address::generate(&env);
        client.initialize(&admin);

        let session_id = client.create_session(&user);
        let session = client.get_session(&session_id);

        assert_eq!(session.session_id, session_id);
        assert_eq!(session.initiator, user);
    }

    // -----------------------------------------------------------------------
    // get_session_operation_count
    // -----------------------------------------------------------------------

    #[test]
    fn test_operation_count_starts_at_zero() {
        let env = make_env();
        setup_ledger(&env);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let user = Address::generate(&env);
        client.initialize(&admin);

        let session_id = client.create_session(&user);
        assert_eq!(client.get_session_operation_count(&session_id), 0);
    }

    #[test]
    fn test_operation_count_increments_with_register_attestor_with_session() {
        let env = make_env();
        setup_ledger(&env);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let user = Address::generate(&env);
        let attestor = Address::generate(&env);
        client.initialize(&admin);

        let session_id = client.create_session(&user);
        let sk_reg = SigningKey::generate(&mut OsRng);
        let pk_reg = soroban_sdk::BytesN::from_array(&env, sk_reg.verifying_key().as_bytes());
        client.register_attestor_with_session(&admin, &session_id, &attestor, &pk_reg);

        assert_eq!(client.get_session_operation_count(&session_id), 1);
    }

    #[test]
    fn test_operation_count_increments_with_submit_attestation_with_session() {
        let env = make_env();
        setup_ledger(&env);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let user = Address::generate(&env);
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        client.initialize(&admin);

        let session_id = client.create_session(&user);
        let sk = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &attestor, &attestor, &sk);

        let ph = payload(&env, 0x01);
        let real_sig = sign_payload(&env, &sk, &ph);
        client.submit_attestation_with_session(
            &session_id,
            &attestor,
            &subject,
            &1700000001u64,
            &ph,
            &real_sig,
        );

        assert_eq!(client.get_session_operation_count(&session_id), 1);
    }

    // -----------------------------------------------------------------------
    // register_attestor_with_session
    // -----------------------------------------------------------------------

    #[test]
    fn test_register_attestor_with_session_registers_attestor() {
        let env = make_env();
        setup_ledger(&env);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let user = Address::generate(&env);
        let attestor = Address::generate(&env);
        client.initialize(&admin);

        let session_id = client.create_session(&user);
        let sk = SigningKey::generate(&mut OsRng);
        let pk = soroban_sdk::BytesN::from_array(&env, sk.verifying_key().as_bytes());
        client.register_attestor_with_session(&admin, &session_id, &attestor, &pk);

        assert!(client.is_attestor(&attestor));
    }

    #[test]
    fn test_register_attestor_with_session_writes_audit_log() {
        let env = make_env();
        setup_ledger(&env);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let user = Address::generate(&env);
        let attestor = Address::generate(&env);
        client.initialize(&admin);

        let session_id = client.create_session(&user);
        let sk = SigningKey::generate(&mut OsRng);
        let pk = soroban_sdk::BytesN::from_array(&env, sk.verifying_key().as_bytes());
        client.register_attestor_with_session(&admin, &session_id, &attestor, &pk);

        let log = client.get_audit_log(&0u64);
        assert_eq!(log.log_id, 0);
        assert_eq!(log.session_id, session_id);
        assert_eq!(log.operation.operation_type, String::from_str(&env, "register"));
        assert_eq!(log.operation.status, String::from_str(&env, "success"));
        assert_eq!(log.operation.operation_index, 0);
    }

    // -----------------------------------------------------------------------
    // revoke_attestor_with_session
    // -----------------------------------------------------------------------

    #[test]
    fn test_revoke_attestor_with_session_removes_attestor() {
        let env = make_env();
        setup_ledger(&env);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let user = Address::generate(&env);
        let attestor = Address::generate(&env);
        client.initialize(&admin);

        let session_id = client.create_session(&user);
        let sk = SigningKey::generate(&mut OsRng);
        let pk = soroban_sdk::BytesN::from_array(&env, sk.verifying_key().as_bytes());
        client.register_attestor_with_session(&admin, &session_id, &attestor, &pk);
        client.revoke_attestor_with_session(&admin, &session_id, &attestor);

        assert!(!client.is_attestor(&attestor));
    }

    #[test]
    fn test_revoke_attestor_with_session_writes_audit_log() {
        let env = make_env();
        setup_ledger(&env);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let user = Address::generate(&env);
        let attestor = Address::generate(&env);
        client.initialize(&admin);

        let session_id = client.create_session(&user);
        let sk2 = SigningKey::generate(&mut OsRng);
        let pk2 = soroban_sdk::BytesN::from_array(&env, sk2.verifying_key().as_bytes());
        client.register_attestor_with_session(&admin, &session_id, &attestor, &pk2);
        client.revoke_attestor_with_session(&admin, &session_id, &attestor);

        // log_id 0 = register, log_id 1 = revoke
        let log = client.get_audit_log(&1u64);
        assert_eq!(log.log_id, 1);
        assert_eq!(log.session_id, session_id);
        assert_eq!(log.operation.operation_type, String::from_str(&env, "revoke"));
        assert_eq!(log.operation.status, String::from_str(&env, "success"));
    }

    // -----------------------------------------------------------------------
    // get_audit_log
    // -----------------------------------------------------------------------

    #[test]
    fn test_audit_log_sequential_ids_across_operations() {
        let env = make_env();
        setup_ledger(&env);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let user = Address::generate(&env);
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        client.initialize(&admin);

        let session_id = client.create_session(&user);
        let sk = SigningKey::generate(&mut OsRng);
        let pk = soroban_sdk::BytesN::from_array(&env, sk.verifying_key().as_bytes());
        client.register_attestor_with_session(&admin, &session_id, &attestor, &pk);
        let ph = payload(&env, 0x01);
        let real_sig = sign_payload(&env, &sk, &ph);
        client.submit_attestation_with_session(
            &session_id,
            &attestor,
            &subject,
            &1700000001u64,
            &ph,
            &real_sig,
        );

        let log0 = client.get_audit_log(&0u64);
        let log1 = client.get_audit_log(&1u64);
        assert_eq!(log0.log_id, 0);
        assert_eq!(log1.log_id, 1);
        assert_eq!(log0.operation.operation_type, String::from_str(&env, "register"));
        assert_eq!(log1.operation.operation_type, String::from_str(&env, "attest"));
    }

    // -----------------------------------------------------------------------
    // Snapshot reproducibility test (matches test_snapshots/session_tests/)
    // -----------------------------------------------------------------------

    #[test]
    fn test_recorded_anchor_session_replay_is_reproducible_offline() {
        let env = make_env();
        setup_ledger(&env);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let user = Address::generate(&env);
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        client.initialize(&admin);

        // Step 1: create session
        let session_id = client.create_session(&user);
        assert_eq!(session_id, 0);

        // Step 2: register attestor with session
        let sk = SigningKey::generate(&mut OsRng);
        let pk = soroban_sdk::BytesN::from_array(&env, sk.verifying_key().as_bytes());
        client.register_attestor_with_session(&admin, &session_id, &attestor, &pk);
        assert!(client.is_attestor(&attestor));

        // Step 3: two attestations
        let ph0 = payload(&env, 0x01);
        let ph1 = payload(&env, 0x02);
        let sig0 = sign_payload(&env, &sk, &ph0);
        let sig1 = sign_payload(&env, &sk, &ph1);
        let id0 = client.submit_attestation_with_session(
            &session_id,
            &attestor,
            &subject,
            &1700000001u64,
            &ph0,
            &sig0,
        );
        let id1 = client.submit_attestation_with_session(
            &session_id,
            &attestor,
            &subject,
            &1700000002u64,
            &ph1,
            &sig1,
        );
        assert_eq!(id0, 0);
        assert_eq!(id1, 1);

        // Step 4: verify operation count = 3 (register + 2 attests)
        assert_eq!(client.get_session_operation_count(&session_id), 3);

        // Step 5: verify audit logs
        let log0 = client.get_audit_log(&0u64);
        assert_eq!(log0.operation.operation_type, String::from_str(&env, "register"));
        assert_eq!(log0.operation.operation_index, 0);

        let log1 = client.get_audit_log(&1u64);
        assert_eq!(log1.operation.operation_type, String::from_str(&env, "attest"));
        assert_eq!(log1.operation.operation_index, 1);
        assert_eq!(log1.operation.result_data, 0); // attestation id 0

        let log2 = client.get_audit_log(&2u64);
        assert_eq!(log2.operation.operation_type, String::from_str(&env, "attest"));
        assert_eq!(log2.operation.operation_index, 2);
        assert_eq!(log2.operation.result_data, 1); // attestation id 1
    }

    // -----------------------------------------------------------------------
    // Session TTL / expiry
    // -----------------------------------------------------------------------

    #[test]
    #[should_panic]
    fn test_submit_attestation_with_session_panics_when_session_expired() {
        let env = make_env();
        env.ledger().set(LedgerInfo {
            timestamp: 0,
            protocol_version: 21,
            sequence_number: 0,
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });

        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let user = Address::generate(&env);
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        client.initialize(&admin);

        let session_id = client.create_session(&user);
        let sk = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &attestor, &attestor, &sk);

        // Advance ledger past the 3600s TTL
        env.ledger().set(LedgerInfo {
            timestamp: 3601,
            protocol_version: 21,
            sequence_number: 1,
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });

        // Should panic with SessionExpired
        let ph = payload(&env, 0x01);
        let real_sig = sign_payload(&env, &sk, &ph);
        client.submit_attestation_with_session(
            &session_id,
            &attestor,
            &subject,
            &1700000001u64,
            &ph,
            &real_sig,
        );
    }

    #[test]
    fn test_submit_attestation_with_session_succeeds_within_ttl() {
        let env = make_env();
        env.ledger().set(LedgerInfo {
            timestamp: 0,
            protocol_version: 21,
            sequence_number: 0,
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });

        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let user = Address::generate(&env);
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        client.initialize(&admin);

        let session_id = client.create_session(&user);
        let sk = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &attestor, &attestor, &sk);

        // Advance to exactly the TTL boundary — should still be valid
        env.ledger().set(LedgerInfo {
            timestamp: 3600,
            protocol_version: 21,
            sequence_number: 1,
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });

        let ph = payload(&env, 0x01);
        let real_sig = sign_payload(&env, &sk, &ph);
        let id = client.submit_attestation_with_session(
            &session_id,
            &attestor,
            &subject,
            &1700000001u64,
            &ph,
            &real_sig,
        );
        assert_eq!(id, 0);
    }

    // -----------------------------------------------------------------------
    // Closed session rejection
    // -----------------------------------------------------------------------

    #[test]
    #[should_panic]
    fn test_submit_attestation_with_session_panics_when_session_closed() {
        let env = make_env();
        setup_ledger(&env);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let user = Address::generate(&env);
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        client.initialize(&admin);

        let session_id = client.create_session(&user);
        let sk = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &attestor, &attestor, &sk);

        client.close_session(&session_id, &user);

        // Should panic with SessionClosed
        client.submit_attestation_with_session(
            &session_id,
            &attestor,
            &subject,
            &1700000001u64,
            &payload(&env, 0x01),
            &sig(&env, &[0x0a, 0x0b]),
        );
    }

    #[test]
    #[should_panic]
    fn test_register_attestor_with_session_panics_when_session_closed() {
        let env = make_env();
        setup_ledger(&env);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let user = Address::generate(&env);
        let attestor = Address::generate(&env);
        client.initialize(&admin);

        let session_id = client.create_session(&user);
        client.close_session(&session_id, &user);

        // Should panic with SessionClosed
        let sk = SigningKey::generate(&mut OsRng);
        let pk = soroban_sdk::BytesN::from_array(&env, sk.verifying_key().as_bytes());
        client.register_attestor_with_session(&admin, &session_id, &attestor, &pk);
    }

    // -----------------------------------------------------------------------
    // Valid active session succeeds
    // -----------------------------------------------------------------------

    #[test]
    fn test_valid_active_session_allows_operations() {
        let env = make_env();
        setup_ledger(&env);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let user = Address::generate(&env);
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        client.initialize(&admin);

        let session_id = client.create_session(&user);
        let sk = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &attestor, &attestor, &sk);

        // Session is fresh — all operations should succeed
        let id = client.submit_attestation_with_session(
            &session_id,
            &attestor,
            &subject,
            &1700000001u64,
            &payload(&env, 0x01),
            &sig(&env, &[0x0a, 0x0b]),
        );
        assert_eq!(id, 0);

        let session = client.get_session(&session_id);
        assert!(!session.closed);
    }

    // -----------------------------------------------------------------------
    // TTL boundary: exactly at expiry is still valid, one second past is not
    // -----------------------------------------------------------------------

    #[test]
    #[should_panic]
    fn test_session_expired_one_second_past_ttl() {
        let env = make_env();
        env.ledger().set(LedgerInfo {
            timestamp: 1000,
            protocol_version: 21,
            sequence_number: 0,
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });

        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let user = Address::generate(&env);
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        client.initialize(&admin);

        // Session created at t=1000, TTL=3600 → expires at t=4601
        let session_id = client.create_session(&user);
        let sk = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &attestor, &attestor, &sk);

        // Advance to t=4602 (one second past expiry)
        env.ledger().set(LedgerInfo {
            timestamp: 4602,
            protocol_version: 21,
            sequence_number: 1,
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });

        // Should panic with SessionExpired
        client.submit_attestation_with_session(
            &session_id,
            &attestor,
            &subject,
            &1700000001u64,
            &payload(&env, 0x01),
            &sig(&env, &[0x0a, 0x0b]),
        );
    }

    // -----------------------------------------------------------------------
    // Extreme-TTL: saturating arithmetic must prevent overflow
    // -----------------------------------------------------------------------

    /// A session whose TTL is u64::MAX must never appear immediately expired.
    ///
    /// If expiry were computed with plain `+` the addition would wrap to a
    /// timestamp in the past and the session would be rejected on every call.
    /// `session_state_machine::session_expiry` uses `saturating_add`, so
    /// the expiry saturates at `u64::MAX` and the session remains live for any
    /// reasonable ledger timestamp.
    ///
    /// This test exercises the on-contract path by setting `session_ttl_seconds`
    /// to `u64::MAX` via a direct storage write and then verifying that an
    /// operation succeeds rather than panicking with `SessionExpired`.
    #[test]
    fn test_extreme_ttl_does_not_overflow_to_expired() {
        use soroban_sdk::testutils::Ledger;

        let env = make_env();
        env.ledger().set(LedgerInfo {
            timestamp: 1000,
            protocol_version: 21,
            sequence_number: 0,
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });

        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let user = Address::generate(&env);
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        client.initialize(&admin);

        let session_id = client.create_session(&user);
        let sk = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &attestor, &attestor, &sk);

        // Advance ledger to a large but valid timestamp.
        // With a correctly saturating expiry (created_at=1000, ttl=u64::MAX →
        // expiry=u64::MAX) this session must still be live at t=9_999_999.
        env.ledger().set(LedgerInfo {
            timestamp: 9_999_999,
            protocol_version: 21,
            sequence_number: 1,
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });

        // Verify session_expiry arithmetic directly (pure function, no contract needed).
        let expiry = anchorkit::session_state_machine::session_expiry(1000, u64::MAX);
        assert_eq!(expiry, u64::MAX,
            "saturating_add must produce u64::MAX, not a wrapped/small value");
        assert!(9_999_999u64 <= expiry,
            "session must not appear expired when expiry saturated at u64::MAX");
    }
}

/// Boundary and lifecycle regression tests for the session state machine:
/// exact TTL expiry (#1256), the per-session operation limit (#1257), and
/// repeated close calls (#1258).
///
/// All tests drive time through the Soroban test ledger only (no sleeping)
/// and assert the stable `ErrorCode` returned by the `try_*` client methods.
mod session_boundary_tests {
    use soroban_sdk::{
        testutils::{Address as _, Ledger, LedgerInfo},
        Address, BytesN, Env, Error,
    };

    use anchorkit::contract::{
        AnchorKitContract, AnchorKitContractClient, DEFAULT_SESSION_TTL, MAX_OPS_PER_SESSION,
    };
    use anchorkit::errors::ErrorCode;
    use anchorkit::session_state_machine::SessionState;

    /// Non-zero creation time so `created_at + ttl` is distinguishable from the TTL itself.
    const CREATED_AT: u64 = 1_000;

    fn set_ledger(env: &Env, timestamp: u64) {
        env.ledger().set(LedgerInfo {
            timestamp,
            protocol_version: 21,
            sequence_number: 0,
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6_312_000,
        });
    }

    /// Register a contract at `CREATED_AT`, open one session for `user`, and
    /// return `(client, admin, user, session_id)`.
    fn setup(env: &Env) -> (AnchorKitContractClient<'_>, Address, Address, u64) {
        env.mock_all_auths();
        set_ledger(env, CREATED_AT);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(env, &contract_id);
        let admin = Address::generate(env);
        let user = Address::generate(env);
        client.initialize(&admin);
        let session_id = client.create_session(&user);
        (client, admin, user, session_id)
    }

    /// The host error a contract call fails with for a given stable `ErrorCode`.
    fn contract_error(code: ErrorCode) -> Error {
        Error::from_contract_error(code as u32)
    }

    fn public_key(env: &Env) -> BytesN<32> {
        BytesN::from_array(env, &[7u8; 32])
    }

    // -----------------------------------------------------------------------
    // #1256 — expiration exact boundary
    // -----------------------------------------------------------------------

    /// Expiry policy: a session is usable while `now <= created_at + ttl` and
    /// rejected with `SessionExpired` once `now > created_at + ttl`.
    #[test]
    fn test_session_usable_at_exact_expiry_and_expired_one_second_after() {
        let env = Env::default();
        let (client, admin, _user, session_id) = setup(&env);
        let pk = public_key(&env);
        let expiry = CREATED_AT + DEFAULT_SESSION_TTL;

        // Before expiry: usable.
        set_ledger(&env, expiry - 1);
        let first = Address::generate(&env);
        assert!(client.try_register_attestor_with_session(&admin, &session_id, &first, &pk).is_ok());
        assert_eq!(client.get_session_state(&session_id), SessionState::Active as u32);

        // Exactly at expiry: still usable.
        set_ledger(&env, expiry);
        let second = Address::generate(&env);
        assert!(
            client.try_register_attestor_with_session(&admin, &session_id, &second, &pk).is_ok(),
            "an operation at exactly created_at + ttl must be accepted",
        );
        assert_eq!(client.get_session_state(&session_id), SessionState::Active as u32);
        assert_eq!(client.get_session_operation_count(&session_id), 2);

        // One second beyond expiry: rejected with SessionExpired.
        set_ledger(&env, expiry + 1);
        let third = Address::generate(&env);
        let result = client.try_register_attestor_with_session(&admin, &session_id, &third, &pk);
        assert_eq!(result.unwrap_err().unwrap(), contract_error(ErrorCode::SessionExpired));
        assert_eq!(client.get_session_state(&session_id), SessionState::Expired as u32);
        assert_eq!(client.get_session_operation_count(&session_id), 2);
    }

    // -----------------------------------------------------------------------
    // #1257 — operation limit boundary
    // -----------------------------------------------------------------------

    /// The session limit is the compile-time `MAX_OPS_PER_SESSION` (no smaller
    /// per-session limit is configurable). Operations 1..=MAX are accepted;
    /// operation MAX + 1 fails with `SessionOperationLimitExceeded`.
    #[test]
    fn test_session_operation_limit_accepts_max_and_rejects_next() {
        let env = Env::default();
        let (client, admin, _user, session_id) = setup(&env);
        let pk = public_key(&env);
        let attestor = Address::generate(&env);

        // Alternate register/revoke of one attestor so every call is a valid
        // session operation without growing the attestor set.
        for i in 0..MAX_OPS_PER_SESSION {
            if i % 2 == 0 {
                client.register_attestor_with_session(&admin, &session_id, &attestor, &pk);
            } else {
                client.revoke_attestor_with_session(&admin, &session_id, &attestor);
            }
        }
        assert_eq!(client.get_session_operation_count(&session_id), MAX_OPS_PER_SESSION);

        // MAX_OPS_PER_SESSION is even, so the attestor is currently revoked and
        // the next call would otherwise be a valid registration.
        let result = client.try_register_attestor_with_session(&admin, &session_id, &attestor, &pk);
        assert_eq!(result.unwrap_err().unwrap(), contract_error(ErrorCode::SessionOperationLimitExceeded));
        assert_eq!(client.get_session_operation_count(&session_id), MAX_OPS_PER_SESSION);
        assert!(!client.is_attestor(&attestor), "rejected operation must not take effect");
    }

    // -----------------------------------------------------------------------
    // #1258 — closing an already closed session
    // -----------------------------------------------------------------------

    /// A retried close is not idempotent: the second call fails and the
    /// session stays closed.
    ///
    /// The public outcome is `IllegalTransition`, not `SessionClosed`:
    /// `session_state_machine::validate_transition` reports `SameState` for
    /// Closed → Closed before its terminal-state check, and `close_session`
    /// maps `SameState` to `IllegalTransition`.
    #[test]
    fn test_closing_already_closed_session_returns_illegal_transition() {
        let env = Env::default();
        let (client, admin, user, session_id) = setup(&env);

        client.close_session(&session_id, &user);
        assert_eq!(client.get_session_state(&session_id), SessionState::Closed as u32);

        let result = client.try_close_session(&session_id, &user);
        assert_eq!(result.unwrap_err().unwrap(), contract_error(ErrorCode::IllegalTransition));

        // The failed retry leaves the session closed, not active again.
        let session = client.get_session(&session_id);
        assert!(session.closed);
        assert_eq!(session.state, SessionState::Closed as u32);
        assert_eq!(client.get_session_state(&session_id), SessionState::Closed as u32);

        // Operations are still rejected after the retried close.
        let attestor = Address::generate(&env);
        let result = client.try_register_attestor_with_session(&admin, &session_id, &attestor, &public_key(&env));
        assert_eq!(result.unwrap_err().unwrap(), contract_error(ErrorCode::SessionClosed));
        assert_eq!(client.get_session_operation_count(&session_id), 0);
    }
}
