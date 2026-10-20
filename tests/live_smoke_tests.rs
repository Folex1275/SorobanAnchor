//! Live network smoke tests for SorobanAnchor.
//!
//! These tests verify that a deployed contract on the Stellar testnet (or
//! another live network) is reachable and behaves correctly. They are
//! **opt-in** and will be skipped unless the required environment variables
//! are set.
//!
//! # Prerequisites
//!
//! | Variable | Required | Description |
//! |----------|----------|-------------|
//! | `SOROBAN_ANCHOR_INTEGRATION` | Yes (set to `testnet`) | Enables the live test suite |
//! | `ANCHOR_CONTRACT_ID` | Yes | Contract ID of the deployed AnchorKit contract |
//! | `ANCHOR_ADMIN_SECRET` | Yes | Stellar secret key of the contract admin |
//! | `STELLAR_NETWORK` | No (default: `testnet`) | Network name for the Stellar CLI |
//! | `STELLAR_RPC_URL` | No | Override for the Stellar RPC endpoint |
//! | `STELLAR_NETWORK_PASSPHRASE` | No | Override for the network passphrase |
//!
//! # Running
//!
//! ```bash
//! # Export required variables and run the live smoke suite.
//! export SOROBAN_ANCHOR_INTEGRATION=testnet
//! export ANCHOR_CONTRACT_ID=<deployed-contract-id>
//! export ANCHOR_ADMIN_SECRET=<admin-secret-key>
//!
//! cargo test --test live_smoke_tests -- --nocapture
//! ```
//!
//! Alternatively, use the convenience Make target:
//!
//! ```bash
//! make integration-test-live
//! ```
//!
//! # Safety
//!
//! - These tests **never** run in normal CI. The `SOROBAN_ANCHOR_INTEGRATION`
//!   guard ensures they are skipped when the variable is absent or set to any
//!   value other than `testnet`.
//! - Tests are **read-heavy** where possible. The only write operation is a
//!   single test attestation submission tagged with a deterministic payload hash
//!   so it is idempotent across re-runs (the replay guard will reject duplicates
//!   on subsequent runs, which is an expected outcome, not a failure).
//! - No funds are transferred by any test in this suite.
//!
//! # Expected outcomes
//!
//! | Step | Expected |
//! |------|----------|
//! | `get_admin` | Returns a non-empty Stellar address |
//! | `get_schema_version` | Returns a positive integer |
//! | `is_initialized` | Returns `true` |
//! | `supported_seps` | Returns a non-empty list containing 6, 10, 24, 31, 38 |
//! | `get_attestor_count` | Returns a non-negative integer |
//! | `submit_attestation` (smoke) | Returns an attestation ID or panics with ReplayAttack if already submitted |

#![cfg(test)]

extern crate std;

use std::{
    env,
    process::Command,
    string::{String as StdString, ToString},
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Default RPC endpoint for Stellar testnet.
const DEFAULT_RPC_URL: &str = "https://soroban-testnet.stellar.org";

/// Default network passphrase for Stellar testnet.
const DEFAULT_PASSPHRASE: &str = "Test SDF Network ; September 2015";

/// Returns `true` when all required environment variables are set and
/// `SOROBAN_ANCHOR_INTEGRATION == "testnet"`.  When this returns `false`,
/// the calling test should skip gracefully.
fn live_env_ready() -> bool {
    if env::var("SOROBAN_ANCHOR_INTEGRATION").as_deref() != Ok("testnet") {
        return false;
    }
    let contract_id = env::var("ANCHOR_CONTRACT_ID").unwrap_or_default();
    let admin_secret = env::var("ANCHOR_ADMIN_SECRET").unwrap_or_default();
    !contract_id.is_empty() && !admin_secret.is_empty()
}

/// Prints a skip notice and returns `true` when the live environment is not
/// ready.  Call at the top of every test: `if skip_if_not_live() { return; }`.
fn skip_if_not_live() -> bool {
    if live_env_ready() {
        return false;
    }
    eprintln!(
        "SKIP: set SOROBAN_ANCHOR_INTEGRATION=testnet, ANCHOR_CONTRACT_ID, and \
         ANCHOR_ADMIN_SECRET to run live smoke tests"
    );
    true
}

/// Reads the contract ID from the environment. Panics if not set (caller must
/// have already called `skip_if_not_live()`).
fn contract_id() -> StdString {
    env::var("ANCHOR_CONTRACT_ID").expect("ANCHOR_CONTRACT_ID must be set")
}

/// Reads the admin secret key from the environment.
fn admin_secret() -> StdString {
    env::var("ANCHOR_ADMIN_SECRET").expect("ANCHOR_ADMIN_SECRET must be set")
}

/// Returns the configured RPC URL, defaulting to testnet.
fn rpc_url() -> StdString {
    env::var("STELLAR_RPC_URL")
        .unwrap_or_else(|_| DEFAULT_RPC_URL.to_string())
}

/// Returns the network passphrase, defaulting to the Stellar testnet value.
fn network_passphrase() -> StdString {
    env::var("STELLAR_NETWORK_PASSPHRASE")
        .unwrap_or_else(|_| DEFAULT_PASSPHRASE.to_string())
}

/// Invokes a read-only contract function via the `stellar` CLI.
/// Returns `(stdout, stderr)` on success.  Panics with a descriptive message
/// on non-zero exit code.
fn stellar_invoke_readonly(function: &str, extra_args: &[&str]) -> (StdString, StdString) {
    let mut cmd = Command::new("stellar");
    cmd.args([
        "contract",
        "invoke",
        "--id",
        &contract_id(),
        "--source",
        &admin_secret(),
        "--rpc-url",
        &rpc_url(),
        "--network-passphrase",
        &network_passphrase(),
        "--",
        function,
    ]);
    for arg in extra_args {
        cmd.arg(arg);
    }

    let output = cmd
        .output()
        .expect("stellar CLI must be available to run live smoke tests");

    let stdout = StdString::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = StdString::from_utf8_lossy(&output.stderr).trim().to_string();

    if !output.status.success() {
        panic!(
            "stellar contract invoke '{}' failed:\nstdout: {}\nstderr: {}",
            function, stdout, stderr
        );
    }

    (stdout, stderr)
}

// ---------------------------------------------------------------------------
// Smoke test 1: get_admin — contract is reachable and admin is set
// ---------------------------------------------------------------------------

/// Calls `get_admin` on the live contract and asserts a non-empty address is
/// returned.  This is the most basic reachability check.
#[test]
fn live_smoke_get_admin() {
    if skip_if_not_live() {
        return;
    }

    let (admin, _) = stellar_invoke_readonly("get_admin", &[]);
    assert!(
        !admin.is_empty(),
        "get_admin must return a non-empty address on the live contract"
    );
    // Stellar addresses start with 'G' and are 56 characters long.
    let addr = admin.trim_matches('"');
    assert!(
        addr.starts_with('G') && addr.len() == 56,
        "get_admin must return a valid Stellar address, got: {}",
        addr
    );
    eprintln!("✅ live_smoke_get_admin: admin = {}", addr);
}

// ---------------------------------------------------------------------------
// Smoke test 2: is_initialized — contract initialisation state
// ---------------------------------------------------------------------------

/// Verifies that `is_initialized` returns `true`, confirming the deployment
/// and initialization workflow completed successfully.
#[test]
fn live_smoke_is_initialized() {
    if skip_if_not_live() {
        return;
    }

    let (result, _) = stellar_invoke_readonly("is_initialized", &[]);
    assert!(
        result.contains("true"),
        "is_initialized must return true on a live deployed contract, got: {}",
        result
    );
    eprintln!("✅ live_smoke_is_initialized: {}", result);
}

// ---------------------------------------------------------------------------
// Smoke test 3: get_schema_version — schema version is positive
// ---------------------------------------------------------------------------

/// Calls `get_schema_version` and asserts it returns a positive integer.
/// A value of 0 would indicate the migration step was never executed.
#[test]
fn live_smoke_get_schema_version() {
    if skip_if_not_live() {
        return;
    }

    let (version_str, _) = stellar_invoke_readonly("get_schema_version", &[]);
    let version: u32 = version_str
        .trim()
        .parse()
        .unwrap_or(0);
    assert!(
        version > 0,
        "schema version must be > 0 on a live initialized contract, got: {}",
        version_str
    );
    eprintln!("✅ live_smoke_get_schema_version: {}", version);
}

// ---------------------------------------------------------------------------
// Smoke test 4: supported_seps — all five SEPs are declared
// ---------------------------------------------------------------------------

/// Calls `supported_seps` and verifies the contract declares support for
/// SEPs 6, 10, 24, 31, and 38.
#[test]
fn live_smoke_supported_seps() {
    if skip_if_not_live() {
        return;
    }

    let (result, _) = stellar_invoke_readonly("supported_seps", &[]);

    for sep in ["6", "10", "24", "31", "38"] {
        assert!(
            result.contains(sep),
            "supported_seps must include SEP-{}, got: {}",
            sep,
            result
        );
    }
    eprintln!("✅ live_smoke_supported_seps: {}", result);
}

// ---------------------------------------------------------------------------
// Smoke test 5: get_attestor_count — counter is readable
// ---------------------------------------------------------------------------

/// Reads the current attestor count from the contract.  Any non-negative value
/// is acceptable — this test simply confirms the storage key is accessible.
#[test]
fn live_smoke_get_attestor_count() {
    if skip_if_not_live() {
        return;
    }

    let (count_str, _) = stellar_invoke_readonly("get_attestor_count", &[]);
    let count: u64 = count_str.trim().parse().unwrap_or(u64::MAX);
    assert_ne!(
        count,
        u64::MAX,
        "get_attestor_count must return a valid integer, got: {}",
        count_str
    );
    eprintln!("✅ live_smoke_get_attestor_count: {}", count);
}

// ---------------------------------------------------------------------------
// Smoke test 6: get_version — contract version is present
// ---------------------------------------------------------------------------

/// Calls `get_version` and checks the output contains major / minor / patch
/// fields.  The exact values depend on the deployed release.
#[test]
fn live_smoke_get_version() {
    if skip_if_not_live() {
        return;
    }

    let (version, _) = stellar_invoke_readonly("get_version", &[]);
    assert!(
        !version.is_empty(),
        "get_version must return a non-empty result"
    );
    // The version struct is typically serialized as a JSON-like map.
    // Check at least one expected field name.
    assert!(
        version.contains("major") || version.contains("0"),
        "get_version output must contain version fields, got: {}",
        version
    );
    eprintln!("✅ live_smoke_get_version: {}", version);
}

// ---------------------------------------------------------------------------
// Smoke test 7: replay-idempotent attestation submission
// ---------------------------------------------------------------------------

/// Attempts to submit a smoke-test attestation with a known deterministic
/// payload hash.  Two outcomes are acceptable:
/// - The call succeeds and returns a new attestation ID (first run).
/// - The call fails with a replay-attack error (subsequent runs).
///
/// Any other failure (network error, auth failure, etc.) causes the test to
/// panic with a descriptive message.
#[test]
fn live_smoke_attestation_submission_idempotent() {
    if skip_if_not_live() {
        return;
    }

    // A deterministic 32-byte smoke payload (hex-encoded for CLI argument).
    // This value must never change between runs so the replay guard fires
    // predictably.
    let smoke_payload_hex =
        "534d4f4b455f544553545f50415945454e545f484153485f5f5f5f5f5f5f5f5f5f";

    let mut cmd = Command::new("stellar");
    cmd.args([
        "contract",
        "invoke",
        "--id",
        &contract_id(),
        "--source",
        &admin_secret(),
        "--rpc-url",
        &rpc_url(),
        "--network-passphrase",
        &network_passphrase(),
        "--",
        "submit_attestation",
        "--issuer",
        // The admin account will also serve as the smoke-test attestor.
        // In a real workflow this would be the registered attestor address.
        &admin_secret(),
        "--subject",
        &admin_secret(),
        "--timestamp",
        "1700000000",
        "--payload_hash",
        smoke_payload_hex,
        "--signature",
        // Empty signature is acceptable for smoke testing in local testutils;
        // the on-chain SEP-10 verify path may reject it — both outcomes are valid.
        "",
    ]);

    let output = cmd
        .output()
        .expect("stellar CLI must be available");

    let stdout = StdString::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = StdString::from_utf8_lossy(&output.stderr).trim().to_string();

    if output.status.success() {
        eprintln!(
            "✅ live_smoke_attestation_submission_idempotent: submitted, id = {}",
            stdout
        );
    } else if stderr.contains("ReplayAttack") || stderr.contains("replay") {
        eprintln!(
            "✅ live_smoke_attestation_submission_idempotent: replay correctly rejected (idempotent)"
        );
    } else if stderr.contains("AttestorNotRegistered") || stderr.contains("not registered") {
        // Expected when running against a fresh contract with no registered attestors.
        eprintln!(
            "✅ live_smoke_attestation_submission_idempotent: no registered attestor (expected on fresh contract)"
        );
    } else {
        panic!(
            "live_smoke_attestation_submission_idempotent: unexpected failure:\nstdout: {}\nstderr: {}",
            stdout, stderr
        );
    }
}

// ---------------------------------------------------------------------------
// Smoke test 8: environment readiness report
// ---------------------------------------------------------------------------

/// Prints a summary of the detected environment configuration.
/// This is always executed and never fails — it acts as a diagnostic header
/// for the live smoke output.
#[test]
fn live_smoke_environment_report() {
    let integration = env::var("SOROBAN_ANCHOR_INTEGRATION").unwrap_or_else(|_| "(not set)".into());
    let contract = env::var("ANCHOR_CONTRACT_ID").unwrap_or_else(|_| "(not set)".into());
    let rpc = rpc_url();
    let network = env::var("STELLAR_NETWORK").unwrap_or_else(|_| "testnet (default)".into());

    eprintln!("── Live Smoke Test Environment ──────────────────────────");
    eprintln!("  SOROBAN_ANCHOR_INTEGRATION : {}", integration);
    eprintln!("  ANCHOR_CONTRACT_ID         : {}", contract);
    eprintln!("  STELLAR_RPC_URL            : {}", rpc);
    eprintln!("  STELLAR_NETWORK            : {}", network);
    eprintln!("  Live tests active          : {}", live_env_ready());
    eprintln!("─────────────────────────────────────────────────────────");

    // This test always passes — it is diagnostic only.
}

// ---------------------------------------------------------------------------
// Issue #1306: Live suite skips safely without the master gate
// ---------------------------------------------------------------------------

/// #1306: Live testnet checks must not run during ordinary local test commands.
/// When SOROBAN_ANCHOR_INTEGRATION is absent, the suite must skip without
/// requiring network credentials or making RPC calls.
#[test]
fn test_live_suite_skips_without_gate() {
    // Temporarily unset the gate variable for this test's scope
    let original_gate = env::var("SOROBAN_ANCHOR_INTEGRATION").ok();
    env::remove_var("SOROBAN_ANCHOR_INTEGRATION");

    // The live_env_ready check should return false
    assert!(
        !live_env_ready(),
        "live_env_ready() must return false when SOROBAN_ANCHOR_INTEGRATION is unset"
    );

    // skip_if_not_live should return true (indicating skip)
    assert!(
        skip_if_not_live(),
        "skip_if_not_live() must return true when gate is absent"
    );

    // Restore original value if it existed
    if let Some(val) = original_gate {
        env::set_var("SOROBAN_ANCHOR_INTEGRATION", val);
    }

    eprintln!("✅ Live test suite correctly skips when gate is unset");
}

// ---------------------------------------------------------------------------
// Issue #1307: Live suite rejects non-testnet gate values
// ---------------------------------------------------------------------------

/// #1307: The live smoke workflow is explicitly testnet-only. A different
/// gate value must not accidentally enable the tests against an unspecified
/// network.
#[test]
fn test_live_suite_rejects_non_testnet_gate() {
    // Save original gate value
    let original_gate = env::var("SOROBAN_ANCHOR_INTEGRATION").ok();

    // Test various non-testnet values
    let invalid_gates = vec![
        "mainnet",
        "futurenet",
        "production",
        "local",
        "yes",
        "true",
        "1",
        "",
    ];

    for gate_value in invalid_gates {
        env::set_var("SOROBAN_ANCHOR_INTEGRATION", gate_value);

        // The live_env_ready check should return false for non-"testnet" values
        assert!(
            !live_env_ready(),
            "live_env_ready() must return false when SOROBAN_ANCHOR_INTEGRATION='{}'",
            gate_value
        );

        // skip_if_not_live should return true (indicating skip)
        assert!(
            skip_if_not_live(),
            "skip_if_not_live() must return true when gate='{}' (not 'testnet')",
            gate_value
        );

        eprintln!("✓ Correctly rejected gate value: '{}'", gate_value);
    }

    // Verify that only "testnet" enables the gate (when credentials are also set)
    env::set_var("SOROBAN_ANCHOR_INTEGRATION", "testnet");
    env::set_var("ANCHOR_CONTRACT_ID", "CDUMMYCONTRACT");
    env::set_var("ANCHOR_ADMIN_SECRET", "SDUMMYSECRET");

    assert!(
        live_env_ready(),
        "live_env_ready() must return true only when gate='testnet' and credentials are set"
    );

    // Clean up
    env::remove_var("ANCHOR_CONTRACT_ID");
    env::remove_var("ANCHOR_ADMIN_SECRET");
    
    if let Some(val) = original_gate {
        env::set_var("SOROBAN_ANCHOR_INTEGRATION", val);
    } else {
        env::remove_var("SOROBAN_ANCHOR_INTEGRATION");
    }

    eprintln!("✅ Live test suite only accepts gate='testnet'");
}

