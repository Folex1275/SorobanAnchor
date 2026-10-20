//! Boundary tests for the attestor registration capacity limit (#1253).
//!
//! Registers attestors up to the configured `max_attestors` and verifies that
//! the next registration — carrying a fully valid SEP-10 token so that the
//! capacity check is actually reached — fails with `AttestorCapacityExceeded`
//! without mutating storage.

#![cfg(test)]

mod sep10_test_util;

mod attestor_capacity_boundary_tests {
    use soroban_sdk::{
        testutils::{Address as _, Ledger, LedgerInfo},
        Address, Bytes, BytesN, Env, Error, String,
    };
    use ed25519_dalek::SigningKey;

    use anchorkit::contract::{AnchorKitContract, AnchorKitContractClient, CapacityConfig};
    use anchorkit::errors::ErrorCode;
    use crate::sep10_test_util::{build_sep10_jwt, register_attestor_with_sep10};

    /// Small configured limit keeps the test fast in standard CI.
    const MAX_ATTESTORS: u64 = 3;

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

    fn setup(env: &Env) -> AnchorKitContractClient<'_> {
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(env, &contract_id);
        client.initialize(&Address::generate(env));
        client.set_capacity_config(&CapacityConfig {
            max_attestors: MAX_ATTESTORS,
            max_cache_entries: 10,
        });
        client
    }

    /// Deterministic signing key for the `i`-th attestor.
    fn signing_key(i: u8) -> SigningKey {
        SigningKey::from_bytes(&[i + 1; 32])
    }

    #[test]
    fn registration_at_capacity_succeeds_and_next_is_rejected() {
        let env = make_env();
        let client = setup(&env);

        // Fill the registry up to and including the final allowed slot.
        for i in 0..MAX_ATTESTORS as u8 {
            let attestor = Address::generate(&env);
            register_attestor_with_sep10(&env, &client, &attestor, &attestor, &signing_key(i));
            assert!(client.is_attestor(&attestor));
            assert_eq!(client.get_attestor_count(), u64::from(i) + 1);
        }
        assert_eq!(client.get_attestor_count(), MAX_ATTESTORS);

        // Build a registration that would otherwise succeed, so the only
        // reason for rejection is the capacity limit.
        let sk = signing_key(MAX_ATTESTORS as u8);
        let excess = Address::generate(&env);
        client.set_sep10_jwt_verifying_key(
            &excess,
            &Bytes::from_slice(&env, sk.verifying_key().as_bytes()),
        );
        let sub: std::string::String = excess.to_string().to_string();
        let exp = env.ledger().timestamp() + 86_400;
        let token = String::from_str(&env, build_sep10_jwt(&sk, &sub, exp).as_str());
        let pk = BytesN::from_array(&env, sk.verifying_key().as_bytes());

        let err = client
            .try_register_attestor(&excess, &token, &excess, &pk)
            .expect_err("registration beyond capacity must be rejected")
            .expect("rejection must be a contract error, not a host failure");
        assert_eq!(err, Error::from_contract_error(ErrorCode::AttestorCapacityExceeded as u32));

        // The rejected registration leaves storage untouched.
        assert!(!client.is_attestor(&excess));
        assert_eq!(client.get_attestor_count(), MAX_ATTESTORS);
    }
}
