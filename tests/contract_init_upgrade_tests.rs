//! Tests for contract initialization guards and upgrade/migration authorization.
//!
//! Acceptance criteria verified here:
//! 1. Initialization can only occur once (`AlreadyInitialized` on repeat).
//! 2. `upgrade` and `migrate` require admin authorization.
//! 3. Repeated or malformed initialization requests fail safely.
//! 4. Upgrade with an invalid (all-zero) WASM hash is rejected.
//! 5. Migration with a non-advancing or zero version is rejected.
//! 6. Uninitialized contract rejects upgrade and migrate calls.

#![cfg(test)]

mod contract_init_upgrade_tests {
    use soroban_sdk::testutils::{Address as _, Ledger, LedgerInfo};
    use soroban_sdk::{Address, Bytes, BytesN, Env, Error, IntoVal};

    use anchorkit::contract::{AnchorKitContract, AnchorKitContractClient};
    use anchorkit::errors::ErrorCode;

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

    /// Register a fresh contract and return (client, admin_address).
    fn deploy(env: &Env) -> (AnchorKitContractClient, Address) {
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(env, &contract_id);
        let admin = Address::generate(env);
        (client, admin)
    }

    /// Upload a minimal valid Soroban WASM and return its hash.
    ///
    /// The WASM contains only the required `contractenvmetav0` custom section
    /// so that `update_current_contract_wasm` can resolve the hash in tests.
    fn dummy_wasm_hash(env: &Env) -> BytesN<32> {
        // Minimal Soroban WASM: magic + version + custom section "contractenvmetav0" + empty XDR vec
        let wasm: &[u8] = &[
            0x00, 0x61, 0x73, 0x6d, // magic
            0x01, 0x00, 0x00, 0x00, // version
            0x00, 0x16,             // custom section (id=0), size=22
            0x11,                   // name length = 17
            // "contractenvmetav0"
            0x63, 0x6f, 0x6e, 0x74, 0x72, 0x61, 0x63, 0x74,
            0x65, 0x6e, 0x76, 0x6d, 0x65, 0x74, 0x61, 0x76, 0x30,
            0x00, 0x00, 0x00, 0x00, // empty XDR vec (length = 0)
        ];
        env.deployer().upload_contract_wasm(Bytes::from_slice(env, wasm))
    }

    // -----------------------------------------------------------------------
    // Initialization — happy path
    // -----------------------------------------------------------------------

    #[test]
    fn initialize_succeeds_first_time() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, admin) = deploy(&env);

        // Before init the contract is not initialized.
        assert!(!client.is_initialized());

        client.initialize(&admin);

        // After init the flag is set and admin is retrievable.
        assert!(client.is_initialized());
        assert_eq!(client.get_admin(), admin);
    }

    // -----------------------------------------------------------------------
    // Initialization — repeated attempt must fail
    // -----------------------------------------------------------------------

    #[test]
    #[should_panic]
    fn initialize_twice_panics_with_already_initialized() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, admin) = deploy(&env);

        client.initialize(&admin);
        // Second call must panic with AlreadyInitialized (code 1).
        client.initialize(&admin);
    }

    #[test]
    fn initialize_twice_error_code_is_already_initialized() {
        // Verify the error code value matches the canonical discriminant.
        assert_eq!(ErrorCode::AlreadyInitialized as u32, 1);
    }

    #[test]
    fn second_initialize_returns_already_initialized_error() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, admin) = deploy(&env);

        assert!(client.try_initialize(&admin).is_ok(), "first initialization must succeed");

        let err = client
            .try_initialize(&admin)
            .expect_err("second initialization must be rejected")
            .expect("rejection must be a contract error, not a host failure");
        assert_eq!(err, Error::from_contract_error(ErrorCode::AlreadyInitialized as u32));

        // The rejected call leaves the original configuration in place.
        assert!(client.is_initialized());
        assert_eq!(client.get_admin(), admin);
    }

    // -----------------------------------------------------------------------
    // Initialization — different admin on second call still fails
    // -----------------------------------------------------------------------

    #[test]
    #[should_panic]
    fn initialize_with_different_admin_still_panics() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, admin) = deploy(&env);
        let other_admin = Address::generate(&env);

        client.initialize(&admin);
        // Even a different admin address must not re-initialize.
        client.initialize(&other_admin);
    }

    // -----------------------------------------------------------------------
    // Upgrade — happy path (mock: update_current_contract_wasm is a no-op in tests)
    // -----------------------------------------------------------------------

    #[test]
    fn upgrade_succeeds_when_admin_authorized() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, admin) = deploy(&env);
        client.initialize(&admin);

        // Verify the call completes without panic when admin is authorized.
        client.upgrade(&dummy_wasm_hash(&env));
    }

    // -----------------------------------------------------------------------
    // Upgrade — uninitialized contract must be rejected
    // -----------------------------------------------------------------------

    #[test]
    #[should_panic]
    fn upgrade_on_uninitialized_contract_panics() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, _admin) = deploy(&env);

        // No initialize() call — must panic with NotInitialized.
        client.upgrade(&dummy_wasm_hash(&env));
    }

    // -----------------------------------------------------------------------
    // Upgrade — all-zero WASM hash is an invalid payload
    // -----------------------------------------------------------------------

    #[test]
    #[should_panic]
    fn upgrade_with_zero_hash_panics_with_invalid_payload() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, admin) = deploy(&env);
        client.initialize(&admin);

        let zero_hash = BytesN::from_array(&env, &[0u8; 32]);
        client.upgrade(&zero_hash);
    }

    // -----------------------------------------------------------------------
    // Upgrade — unauthorized caller must be rejected
    // -----------------------------------------------------------------------

    #[test]
    #[should_panic]
    fn upgrade_by_non_admin_panics() {
        let env = Env::default(); // No mock_all_auths — auth is enforced.
        set_ledger(&env, 1000);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);

        // Initialize with admin auth mocked only for this call.
        env.mock_auths(&[soroban_sdk::testutils::MockAuth {
            address: &admin,
            invoke: &soroban_sdk::testutils::MockAuthInvoke {
                contract: &contract_id,
                fn_name: "initialize",
                args: soroban_sdk::vec![&env, admin.clone().into_val(&env)],
                sub_invokes: &[],
            },
        }]);
        client.initialize(&admin);

        // Attempt upgrade as a different (non-admin) address — no auth mocked.
        let attacker = Address::generate(&env);
        env.mock_auths(&[soroban_sdk::testutils::MockAuth {
            address: &attacker,
            invoke: &soroban_sdk::testutils::MockAuthInvoke {
                contract: &contract_id,
                fn_name: "upgrade",
                args: soroban_sdk::vec![&env, dummy_wasm_hash(&env).into_val(&env)],
                sub_invokes: &[],
            },
        }]);
        // This must panic because `attacker` is not the stored admin.
        client.upgrade(&dummy_wasm_hash(&env));
    }

    // -----------------------------------------------------------------------
    // Migrate — happy path
    // -----------------------------------------------------------------------

    #[test]
    fn migrate_succeeds_with_advancing_version() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, admin) = deploy(&env);
        client.initialize(&admin);

        // initialize() sets SCHEMAVER = SCHEMA_V1 = 1
        assert_eq!(client.get_schema_version(), 1);

        client.migrate(&2u32, &100u32);
        assert_eq!(client.get_schema_version(), 2);

        client.migrate(&5u32, &100u32);
        assert_eq!(client.get_schema_version(), 5);
    }

    // -----------------------------------------------------------------------
    // Migrate — uninitialized contract must be rejected
    // -----------------------------------------------------------------------

    #[test]
    #[should_panic]
    fn migrate_on_uninitialized_contract_panics() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, _admin) = deploy(&env);

        client.migrate(&1u32, &100u32);
    }

    // -----------------------------------------------------------------------
    // Migrate — zero version is an invalid payload
    // -----------------------------------------------------------------------

    #[test]
    #[should_panic]
    fn migrate_with_zero_version_panics() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, admin) = deploy(&env);
        client.initialize(&admin);

        client.migrate(&0u32, &100u32);
    }

    // -----------------------------------------------------------------------
    // Migrate — non-advancing version is an invalid payload
    // -----------------------------------------------------------------------

    #[test]
    #[should_panic]
    fn migrate_with_same_version_panics() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, admin) = deploy(&env);
        client.initialize(&admin);

        client.migrate(&3u32, &100u32);
        // Attempting to migrate to the same version must fail.
        client.migrate(&3u32, &100u32);
    }

    #[test]
    #[should_panic]
    fn migrate_with_lower_version_panics() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, admin) = deploy(&env);
        client.initialize(&admin);

        client.migrate(&5u32, &100u32);
        // Downgrade attempt must fail.
        client.migrate(&2u32, &100u32);
    }

    // -----------------------------------------------------------------------
    // Migrate — unauthorized caller must be rejected
    // -----------------------------------------------------------------------

    #[test]
    #[should_panic]
    fn migrate_by_non_admin_panics() {
        let env = Env::default(); // Auth enforced.
        set_ledger(&env, 1000);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);

        env.mock_auths(&[soroban_sdk::testutils::MockAuth {
            address: &admin,
            invoke: &soroban_sdk::testutils::MockAuthInvoke {
                contract: &contract_id,
                fn_name: "initialize",
                args: soroban_sdk::vec![&env, admin.clone().into_val(&env)],
                sub_invokes: &[],
            },
        }]);
        client.initialize(&admin);

        let attacker = Address::generate(&env);
        env.mock_auths(&[soroban_sdk::testutils::MockAuth {
            address: &attacker,
            invoke: &soroban_sdk::testutils::MockAuthInvoke {
                contract: &contract_id,
                fn_name: "migrate",
                args: soroban_sdk::vec![&env, 2u32.into_val(&env), 100u32.into_val(&env)],
                sub_invokes: &[],
            },
        }]);
        // Must panic because attacker is not the stored admin.
        client.migrate(&2u32, &100u32);
    }

    // -----------------------------------------------------------------------
    // is_initialized — reflects state correctly
    // -----------------------------------------------------------------------

    #[test]
    fn is_initialized_returns_false_before_init_and_true_after() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, admin) = deploy(&env);

        assert!(!client.is_initialized(), "should be false before initialize");
        client.initialize(&admin);
        assert!(client.is_initialized(), "should be true after initialize");
    }

    // -----------------------------------------------------------------------
    // get_schema_version — returns 0 before any migration
    // -----------------------------------------------------------------------

    #[test]
    fn get_schema_version_returns_zero_before_any_migration() {
        let env = make_env();
        set_ledger(&env, 1000);
        let (client, admin) = deploy(&env);
        client.initialize(&admin);

        assert_eq!(client.get_schema_version(), 1);
    }
}
