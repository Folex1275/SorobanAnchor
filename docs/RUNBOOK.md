# SorobanAnchor Production Runbook

This document describes the complete lifecycle for building, deploying, validating, upgrading, and recovering SorobanAnchor contracts in production.

---

## Table of Contents

1. [Pre-Deployment Preparation](#pre-deployment-preparation)
2. [Build and Packaging](#build-and-packaging)
3. [Deployment](#deployment)
4. [Post-Deployment Validation](#post-deployment-validation)
5. [Upgrade Procedure](#upgrade-procedure)
6. [Admin Capability Management](#admin-capability-management)
7. [Failure Recovery / Rollback](#failure-recovery--rollback)
8. [Troubleshooting Common Issues](#troubleshooting-common-issues)

---

## Pre-Deployment Preparation

### Prerequisites

Ensure the following tools are installed:
- Rust 1.75+ with `wasm32-unknown-unknown` target
- Python 3.7+ (for config validation)
- `soroban-cli` (for contract deployment)
- Binaryen (optional, for WASM optimization)

### Environment Variables

Set the following environment variables:
```bash
# For testnet
export SOROBAN_NETWORK=testnet
export SOROBAN_RPC_URL=https://soroban-testnet.stellar.org:443
export SOROBAN_NETWORK_PASSPHRASE="Test SDF Network ; September 2015"

# For mainnet
# export SOROBAN_NETWORK=mainnet
# export SOROBAN_RPC_URL=https://soroban-mainnet.stellar.org:443
# export SOROBAN_NETWORK_PASSPHRASE="Public Global Stellar Network ; September 2015"

export ANCHOR_ADMIN_SECRET=<your-admin-secret-key>
```

### Configuration Validation

Run pre-deployment validation:
```bash
./scripts/pre_deploy_validate.sh
```

This validates all config files against the schema and checks dependencies.

You can also run the offline validator directly, without any network access,
to check config files before the full pre-deploy suite:

```bash
# Validate all files in configs/
anchorkit offline validate

# Validate a single file
anchorkit offline validate --config configs/my-anchor.toml
```

The offline validator checks JSON schema compliance, required fields, and
cross-field references (duplicate attestor names, unknown attestors referenced
in operation templates, etc.).  It makes no network calls and is safe to run
in air-gapped environments.

See also: `anchorkit offline validate --help` for accepted formats and options.

---

## Build and Packaging

### Build Steps

1. Clean previous builds:
```bash
cargo clean
```

2. Build release artifacts:
```bash
make release
```
This executes `scripts/package_release.sh` which:
- Installs required Rust targets
- Builds native CLI
- Builds optimized WASM contract
- Creates release bundle

3. Validate the release bundle:
```bash
make release-validate
```

### Artifacts

The release bundle in `dist/anchorkit-<VERSION>/` contains:
- `anchorkit` - CLI binary
- `anchorkit.wasm` - Optimized Soroban contract
- `schemas/config_schema.json` - Config schema
- `configs/` - Example anchor configurations
- `docs/` - Documentation

---

## Deployment

### Mainnet Confirmation

Deploying to mainnet requires an explicit confirmation step to prevent
accidental production deployments.

**Interactive (default):** When `--network mainnet` is used without `--yes` or
`--no-interactive`, the CLI prints the following prompt to stderr and waits for
input:

```
⚠️  You are about to deploy to MAINNET. Continue? [y/N]:
```

Only `y` or `yes` (case-insensitive) proceeds. Any other input, an empty
reply, or pressing Enter on the default `N` aborts the deploy with exit code 1
and the message:

```
Aborted: mainnet deployment not confirmed.
hint: re-run with --yes to skip this prompt in scripted/CI environments.
```

**Non-interactive / automation:** Pass `--yes` to bypass the prompt entirely.
This is the correct approach for CI pipelines, deployment scripts, and any
environment where stdin is not a terminal:

```bash
./target/release/anchorkit deploy --network mainnet --yes
```

`--no-interactive` also bypasses the prompt (it suppresses all interactive
prompts tool-wide), but `--yes` is preferred for mainnet deploys because it
makes the intent explicit.

> **Note:** If the deploy runs in an environment where stdin is unavailable
> (a subprocess with no TTY, a container with closed stdin, etc.) and `--yes`
> is not supplied, `read_line` will fail and the deployment will be aborted.
> Always pass `--yes` in automated pipelines.

**Non-mainnet networks** (testnet, futurenet, custom profiles) never show this
prompt regardless of flags.

### Initial Deployment

1. Verify the WASM checksum:
```bash
sha256sum dist/anchorkit-<VERSION>/anchorkit.wasm
```

2. Deploy using the CLI:
```bash
# testnet (no confirmation required)
./target/release/anchorkit deploy --network testnet

# mainnet — interactive confirmation
./target/release/anchorkit deploy --network mainnet

# mainnet — non-interactive / CI
./target/release/anchorkit deploy --network mainnet --yes
```

3. Initialize the contract with admin address:
```bash
# Using soroban-cli
soroban contract invoke \
  --id <deployed-contract-id> \
  --source $ANCHOR_ADMIN_SECRET \
  --network $SOROBAN_NETWORK \
  -- \
  initialize \
  --admin <admin-address>
```

### Record Deployment Details

Save the following information:
- Contract ID
- Deployment block height
- WASM SHA-256 checksum
- Admin address

---

## Post-Deployment Validation

1. Verify contract initialization:
```bash
soroban contract invoke \
  --id <contract-id> \
  --source $ANCHOR_ADMIN_SECRET \
  --network $SOROBAN_NETWORK \
  -- \
  get_admin
```

2. Run health checks:
```bash
./target/release/anchorkit doctor
```

3. Test basic functionality:
```bash
# Test registering an attestor (dry run)
./target/release/anchorkit register --address <test-attestor> --services deposits --dry-run
```

---

## Upgrade Procedure

### Pre-Upgrade Steps

1. Create a configuration snapshot before any change:
```bash
soroban contract invoke \
  --id <contract-id> \
  --source $ANCHOR_ADMIN_SECRET \
  --network $SOROBAN_NETWORK \
  -- \
  snapshot_services \
  --caller <admin-address> \
  --anchor <anchor-address> \
  --services '[1,2,3]' \
  --description '"pre_upgrade_$(date +%Y%m%d)"'
```

2. Build the new version:
```bash
make release
```

3. Verify the new WASM checksum against published release notes — **never upgrade with an unverified hash**.

### Perform Upgrade

The upgrade is a two-step process: `upgrade` (swaps the WASM binary) followed by `migrate` (advances the schema version). Both steps require admin authorization.

**Step 1 — Install the new WASM:**
```bash
soroban contract install \
  --wasm dist/anchorkit-<NEW-VERSION>/anchorkit.wasm \
  --source $ANCHOR_ADMIN_SECRET \
  --network $SOROBAN_NETWORK
# Capture the printed WASM hash — you will need it in step 2.
```

**Step 2 — Upgrade the live contract:**
```bash
soroban contract invoke \
  --id <contract-id> \
  --source $ANCHOR_ADMIN_SECRET \
  --network $SOROBAN_NETWORK \
  -- \
  upgrade \
  --new_wasm_hash <new-wasm-hash>
```

> The contract validates that `new_wasm_hash` is **not all-zero bytes** before applying the upgrade. A zeroed hash causes an immediate `ValidationError` (code 15) before any state is modified.

**Step 3 — Run the schema migration (if the new version adds one):**
```bash
soroban contract invoke \
  --id <contract-id> \
  --source $ANCHOR_ADMIN_SECRET \
  --network $SOROBAN_NETWORK \
  -- \
  migrate \
  --new_schema_version <target-version> \
  --batch_size 100
```

If the migration processes records in batches (e.g., v1→v2 quote rewrite), re-run the command until `get_schema_version` returns the target version:

```bash
# Check current version
soroban contract invoke --id <contract-id> --network $SOROBAN_NETWORK -- get_schema_version

# Keep running until version matches target
while [ "$(soroban contract invoke --id <contract-id> --network $SOROBAN_NETWORK -- get_schema_version)" != "<target-version>" ]; do
  soroban contract invoke --id <contract-id> --source $ANCHOR_ADMIN_SECRET --network $SOROBAN_NETWORK -- migrate --new_schema_version <target-version> --batch_size 100
done
```

> `migrate` rejects any `new_schema_version` higher than the highest version the current WASM binary understands. This prevents accidentally committing a schema the code cannot interpret.

### Post-Upgrade Validation

Repeat all steps in [Post-Deployment Validation](#post-deployment-validation).

---

## Admin Capability Management

AnchorKit uses a fine-grained capability model in addition to the coarse-grained role model. The primary admin implicitly holds every capability and role. Delegates can be granted individual capabilities without receiving a full admin role.

### Capability reference

| Capability | Numeric | Grants access to |
|-----------|---------|-----------------|
| `UpgradeContract` | 0 | `upgrade` |
| `MigrateSchema` | 1 | `migrate` |
| `SetCacheConfig` | 2 | `set_cache_config`, `set_governance_config` |
| `ManageAttestors` | 3 | `register_attestor`, `revoke_attestor` and session variants |
| `ManageKyc` | 4 | `approve_kyc`, `reject_kyc` |
| `ManageCacheEntries` | 5 | All `cache_*` and `refresh_*_cache*` methods |
| `ToggleServices` | 6 | `enable_service`, `disable_service`, `snapshot_services`, `rollback_services` |
| `SetRateLimits` | 7 | `set_rate_limit_config`, `set_role_rate_limit` |
| `SetJwtConfig` | 8 | `set_sep10_jwt_verifying_key`, `rotate_sep10_key`, `set_jwt_max_len`, `set_jwt_skew` |
| `ManageAnchorMetadata` | 9 | `set_anchor_metadata`, `reactivate_anchor`, `blacklist_anchor`, `unblacklist_anchor` |

### Granting a capability

```bash
soroban contract invoke \
  --id <contract-id> \
  --source $ANCHOR_ADMIN_SECRET \
  --network $SOROBAN_NETWORK \
  -- \
  grant_capability \
  --grantee <delegate-address> \
  --capability 6   # ToggleServices
```

### Revoking a capability

```bash
soroban contract invoke \
  --id <contract-id> \
  --source $ANCHOR_ADMIN_SECRET \
  --network $SOROBAN_NETWORK \
  -- \
  revoke_capability \
  --grantee <delegate-address> \
  --capability 6
```

### Checking a capability

```bash
soroban contract invoke \
  --id <contract-id> \
  --network $SOROBAN_NETWORK \
  -- \
  has_capability \
  --address <delegate-address> \
  --capability 6
```

### Authorization failure behaviour

All privilege failures return `Unauthorized` (code 28) regardless of whether the check was a role check, a capability check, or a raw admin check. This makes authorization failures consistent and unambiguous in logs and monitoring.

---

## Failure Recovery / Rollback

### Immediate Actions

If a deployment or upgrade causes issues:

1. **Pause affected services** (using the service toggle API):
```bash
soroban contract invoke \
  --id <contract-id> \
  --source $ANCHOR_ADMIN_SECRET \
  --network $SOROBAN_NETWORK \
  -- \
  disable_service \
  --caller <admin-address> \
  --anchor <anchor-address> \
  --service_code 1   # repeat for each service code
```

2. **Collect diagnostic information**:
   - Error logs
   - Transaction hashes
   - Contract state snapshots

### Rollback Procedure

1. Locate the previous release bundle in `dist/`.

2. Reinstall the previous WASM:
```bash
soroban contract install \
  --wasm dist/anchorkit-<OLD-VERSION>/anchorkit.wasm \
  --source $ANCHOR_ADMIN_SECRET \
  --network $SOROBAN_NETWORK
```

3. Rollback the contract:
```bash
soroban contract invoke \
  --id <contract-id> \
  --source $ANCHOR_ADMIN_SECRET \
  --network $SOROBAN_NETWORK \
  -- \
  upgrade \
  --wasm_hash <old-wasm-hash>
```

4. Restore service configuration from snapshot:
```bash
soroban contract invoke \
  --id <contract-id> \
  --source $ANCHOR_ADMIN_SECRET \
  --network $SOROBAN_NETWORK \
  -- \
  rollback_services \
  --caller <admin-address> \
  --snapshot_id <snapshot-id>
```

5. Verify rollback with validation steps.

---

## Migration Framework

All schema upgrades are managed through the formal migration framework (`src/migration.rs`). This ensures every upgrade is validated, recorded, and auditable.

### How it works

1. `initialize()` stamps the initial schema version (`V1 = 1`) using `migration::set_version`.
2. `migrate(new_schema_version, batch_size)` validates the target version against the registered migration step table, runs the data transformation, then calls `migration::commit_version` which atomically advances the stored version **and** writes a `MigrationRecord` to persistent storage.
3. The stored version is never advanced until all data writes are complete. A batch that is still in progress returns early without committing — call `migrate` again to continue.

### Checking migration history

```bash
# Current schema version
soroban contract invoke --id <contract-id> --network $SOROBAN_NETWORK \
  -- get_schema_version

# Number of migrations applied
soroban contract invoke --id <contract-id> --network $SOROBAN_NETWORK \
  -- get_migration_count

# Details of the first migration (index 0)
soroban contract invoke --id <contract-id> --network $SOROBAN_NETWORK \
  -- get_migration_record --idx 0
```

### Schema version table

| Version | Constant | Description |
|---------|----------|-------------|
| 1 | `SCHEMA_V1` | Initial schema — written by `initialize()` |
| 2 | `SCHEMA_V2` | Adds `routing_reason` field to `Quote` records |

### Adding a new migration in future releases

1. Increment `LATEST_SCHEMA_VERSION` in `src/migration.rs`.
2. Add a new `MigrationStep::ToV<N>` variant with `required_from`, `produces`, and `label` implementations.
3. Add the step to the `ALL_STEPS` slice.
4. Implement the data transformation in `AnchorKitContract::migrate` in `contract.rs`.
5. The framework will automatically enforce version ordering and record the migration history.

---

## Troubleshooting Common Issues

### Understanding Readiness vs Liveness

AnchorKit health monitoring distinguishes between two concepts that serve different operational purposes:

**Readiness** indicates whether the service is ready to accept and process requests:
- Gates traffic routing decisions
- Failed readiness checks should remove the service from load balancers
- Checks: contract initialization state, admin configuration, rate limiter state
- Query via: `is_initialized()`, `get_contract_diagnostics()`

**Liveness** indicates whether the process is running and not deadlocked:
- Determines if the process needs to be restarted
- Failed liveness checks should trigger process restarts, not traffic rerouting
- Checks: basic health score, endpoint reachability, API responsiveness
- Query via: `build_health_report()`, `build_health_snapshot()`

**Key difference**: A service can be live (process running) but not ready (not initialized), or ready (initialized) but unhealthy (degraded performance). Use readiness checks for traffic management and liveness checks for process management.

**In practice**:
- K8s `readinessProbe`: Query `is_initialized()` — only route traffic when `true`
- K8s `livenessProbe`: Query health endpoint — restart if unresponsive
- Monitoring dashboards: Use `build_health_report()` for composite scores and trend analysis

### Issue: Deployment Fails with "Invalid WASM"

**Solution**:
1. Check that you're using `--no-default-features --features wasm`
2. Verify the WASM is optimized with `wasm-opt -Oz`
3. Check the WASM size (Soroban has size limits)

### Issue: Contract Invocation Fails with "Unauthorized"

**Solution**:
1. Verify the source account is the primary admin, or holds the required role/capability for the operation
2. Use `has_role` or `has_capability` to check what the caller currently holds
3. Check the SEP-10 JWT (if applicable) is valid and not expired
4. For service-toggle operations, ensure the caller has `AdminCapability::ToggleServices` (code 6)
5. For KYC operations, ensure the caller has `AdminRole::KycAdmin` (0) or `AdminCapability::ManageKyc` (4)
6. All authorization failures now return `Unauthorized` (code 28) — see the [Contract Functions](./CONTRACT_FUNCTIONS.md) error codes table

### Issue: Service State Not Persisting

**Solution**:
1. Check that `enable_service()` returns `true`
2. Verify the anchor address is correct
3. Check contract storage limits

### Issue: Config Validation Fails

**Solution**:
1. Run `./scripts/validate_all.sh` for detailed errors
2. Check config against `config_schema.json`
3. Ensure required fields are present

---

## References

- [README.md](../README.md)
- [Governance and Security](./governance-and-security.md)
- [Security Review Checklist](./SECURITY_REVIEW_CHECKLIST.md)
- [Reproducible Builds](./REPRODUCIBLE_BUILDS.md)
- [Service Management](./service-management.md)
- [Contract Functions](./CONTRACT_FUNCTIONS.md)

