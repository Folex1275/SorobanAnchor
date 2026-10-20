# AnchorKit Contract API Reference

This document provides comprehensive documentation for all exported contract functions, including arguments, return values, and failure scenarios.

---

## Authorization Model

AnchorKit uses a **three-tier authorization model**:

| Tier | Who | Access |
|------|-----|--------|
| **Primary admin** | Set by `initialize` | All operations unconditionally |
| **Role holders** | Granted via `grant_role` | Scoped set of operations (`KycAdmin`, `AttestorAdmin`, `CacheAdmin`) |
| **Capability holders** | Granted via `grant_capability` | Individual fine-grained operations |

The primary admin implicitly holds every role and capability. Roles and capabilities are additive — an address that holds either the matching role **or** the matching capability may invoke the operation.

### AdminRole → Operations

| Role | Operations |
|------|-----------|
| `KycAdmin` (0) | `approve_kyc`, `reject_kyc` |
| `AttestorAdmin` (1) | `register_attestor_with_session`, `revoke_attestor_with_session` |
| `CacheAdmin` (2) | All `cache_*` and `refresh_*_cache*` methods |

### AdminCapability → Operations

| Capability | Operations |
|-----------|-----------|
| `UpgradeContract` (0) | `upgrade` |
| `MigrateSchema` (1) | `migrate` |
| `SetCacheConfig` (2) | `set_cache_config`, `set_governance_config` |
| `ManageAttestors` (3) | `register_attestor`, `revoke_attestor`, `register_attestor_with_session`, `revoke_attestor_with_session` |
| `ManageKyc` (4) | `approve_kyc`, `reject_kyc` |
| `ManageCacheEntries` (5) | `cache_metadata`, `cache_metadata_swr`, `force_refresh_metadata`, `refresh_metadata_cache`, `cache_capabilities`, `refresh_capabilities_cache` |
| `ToggleServices` (6) | `enable_service`, `disable_service`, `snapshot_services`, `rollback_services` |
| `SetRateLimits` (7) | `set_rate_limit_config`, `set_role_rate_limit` |
| `SetJwtConfig` (8) | `set_sep10_jwt_verifying_key`, `rotate_sep10_key`, `set_jwt_max_len`, `set_jwt_skew` |
| `ManageAnchorMetadata` (9) | `set_anchor_metadata`, `reactivate_anchor`, `blacklist_anchor`, `unblacklist_anchor` |

All authorization failures produce `Unauthorized` (code 28).

---

## Initialization & Admin

### `initialize(env: Env, admin: Address)`

Initialize the contract with an admin address. Must be called **exactly once** — the contract uses a dedicated persistent `INITIALIZED` flag to prevent re-initialization after upgrades.

**Arguments:**
- `env` - Soroban environment context
- `admin` - Address that will have admin privileges (must authorize)

**Returns:** None

**Errors:**
- `AlreadyInitialized` (code 1) - if contract is already initialized

**Side effects:** Writes `INITIALIZED` flag to persistent storage, stores admin address and initial schema version (`SCHEMAVER = 1`) to instance storage.

---

### `is_initialized(env: Env) -> bool`

Check if the contract has been initialized.

**Arguments:**
- `env` - Soroban environment context

**Returns:** `true` if initialized, `false` otherwise

**Errors:** None

---

### `get_admin(env: Env) -> Address`

Retrieve the current admin address.

**Arguments:**
- `env` - Soroban environment context

**Returns:** The admin [`Address`]

**Errors:**
- `NotInitialized` (code 23) - if contract not initialized

---

## Contract Versioning & Upgrades

### `get_version(env: Env) -> ContractVersion`

Get the current contract version and upgrade timestamp.

**Arguments:**
- `env` - Soroban environment context

**Returns:** [`ContractVersion`] with major, minor, patch, and upgraded_at

**Errors:** None

---

### `upgrade(env: Env, new_wasm_hash: BytesN<32>)`

Upgrade the contract WASM code to a new version.

**Authorization:** Primary admin only (role/capability delegation not supported for this operation to prevent accidental privilege escalation).

**Arguments:**
- `env` - Soroban environment context
- `new_wasm_hash` - SHA-256 hash of new WASM bytecode. **Must be non-zero** — a zeroed hash is rejected early with `ValidationError`.

**Returns:** None

**Errors:**
- `NotInitialized` (code 23) - if contract not initialized
- `Unauthorized` (code 28) - if caller is not the primary admin
- `ValidationError` (code 15) - if `new_wasm_hash` is all-zero bytes

**Side effects:**
- Atomically replaces the contract bytecode
- Increments the patch version component
- Records the upgrade timestamp
- Writes the new WASM hash to `OLDHASH` in instance storage
- Emits `UpgradeEvent` with old/new hashes and updated version info
- Writes an audit log entry

---

### `migrate(env: Env, new_schema_version: u32, batch_size: u32)`

Run post-upgrade schema migration and advance the on-chain schema version. Must be called after each WASM upgrade that changes the stored data layout.

**Authorization:** Primary admin only.

**Migration is separated from `upgrade`** to make state transitions explicit and auditable. Each call advances the schema version by exactly one step and returns early (without committing the version) if a batch migration is still in progress.

**Arguments:**
- `env` - Soroban environment context
- `new_schema_version` - Schema version to advance to. Must be:
  - Greater than 0
  - Strictly greater than the currently stored version (`get_schema_version`)
  - Not greater than `SCHEMA_V2` (highest version this binary understands — prevents migrating to a layout the current code cannot interpret)
- `batch_size` - Max quote records to rewrite per call during the v1→v2 migration

**Returns:** None

**Errors:**
- `NotInitialized` (code 23) - if contract not initialized
- `Unauthorized` (code 28) - if caller is not the primary admin
- `ValidationError` (code 15) - if `new_schema_version` is 0 or ≤ current version
- `UnsupportedCapabilityVersion` (code 27) - if `new_schema_version` exceeds the highest known schema version

**Side effects:**
- When a batch migration is incomplete, returns early without writing the new version; caller must invoke again to continue
- On full completion, writes `new_schema_version` to `SCHEMAVER` in instance storage
- Writes audit log entries for batch migration progress and the final version advance

---

### `get_schema_version(env: Env) -> u32`

Get the current on-chain data schema version.

**Arguments:**
- `_env` - Soroban environment context

**Returns:** Current schema version (`1` after `initialize`, `2` after v1→v2 migration). Returns `0` if the contract has never been initialized.

**Errors:** None

---

### `get_migration_count(env: Env) -> u32`

Return the total number of migrations that have been recorded in the persistent history log.

**Arguments:**
- `env` - Soroban environment context

**Returns:** `u32` count of recorded migration steps

**Errors:** None

---

### `get_migration_record(env: Env, idx: u32) -> MigrationRecord`

Return a single migration history record by zero-based index.

**Arguments:**
- `env` - Soroban environment context
- `idx` - Zero-based index into the history log

**Returns:** [`MigrationRecord`] with `from_version`, `to_version`, `applied_at`, `applied_at_ledger`, `label`

**Errors:**
- `ValidationError` (code 15) - if `idx` is out of range

---

## Cache Configuration

### `set_cache_config(env: Env, config: CacheConfig)`

Set the global cache configuration for TTLs.

**Authorization:** Primary admin only. All three TTL fields must be non-zero.

**Arguments:**
- `env` - Soroban environment context
- `config` - [`CacheConfig`] with metadata_ttl_seconds, capabilities_ttl_seconds, swr_ttl_seconds

**Returns:** None

**Errors:**
- `Unauthorized` (code 28) - if caller is not admin
- `ValidationError` (code 15) - if any TTL field is zero

**Side effects:** Updates instance storage with new cache configuration; writes audit log entry

---

### `get_cache_config(env: Env) -> CacheConfig`

Get the current global cache configuration.

**Arguments:**
- `env` - Soroban environment context

**Returns:** [`CacheConfig`] with current TTL settings

**Errors:** None

---

## Request ID Generation

### `generate_request_id(env: Env) -> RequestId`

Generate a deterministic request ID based on ledger timestamp and sequence.

**Arguments:**
- `env` - Soroban environment context

**Returns:** [`RequestId`] with unique ID and creation timestamp

**Errors:** None

---

### `generate_child_request_id(env: Env, root_bytes: Bytes, nonce: u64) -> RequestId`

Generate a child request ID linked to a root request.

**Arguments:**
- `env` - Soroban environment context
- `root_bytes` - The root request ID bytes
- `nonce` - Unique nonce for this child operation

**Returns:** [`RequestId`] derived from root and nonce

**Errors:** None

---

## Attestor Management

### `register_attestor(env: Env, attestor: Address, sep10_token: String, sep10_issuer: Address, public_key: BytesN<32>)`

Register a new attestor with SEP-10 verification.

**Arguments:**
- `env` - Soroban environment context
- `attestor` - Address of the attestor to register
- `sep10_token` - SEP-10 JWT token for verification
- `sep10_issuer` - Issuer address for SEP-10 token validation
- `public_key` - Ed25519 public key for attestation verification

**Returns:** None

**Errors:**
- `AttestorAlreadyRegistered` (code 2) - if attestor already registered
- `InvalidSep10Token` (code 18) - if token is invalid/expired
- `UnauthorizedAttestor` (code 4) - if caller not authorized

**Side effects:** Stores attestor profile and public key

---

### `revoke_attestor(env: Env, attestor: Address)`

Revoke an attestor's registration.

**Arguments:**
- `env` - Soroban environment context
- `attestor` - Address of attestor to revoke

**Returns:** None

**Errors:**
- `AttestorNotRegistered` (code 3) - if attestor not registered
- `UnauthorizedAttestor` (code 4) - if caller not authorized

**Side effects:** Removes attestor from registry

---

### `is_attestor(env: Env, attestor: Address) -> bool`

Check if an address is a registered attestor.

**Arguments:**
- `env` - Soroban environment context
- `attestor` - Address to check

**Returns:** `true` if registered, `false` otherwise

**Errors:** None

---

### `get_attestor_profile(env: Env, attestor: Address) -> AttestorProfile`

Get the complete profile for an attestor.

**Arguments:**
- `env` - Soroban environment context
- `attestor` - Address of attestor

**Returns:** [`AttestorProfile`] with endpoint, webhook, services, enabled status

**Errors:**
- `AttestorProfileNotFound` (code 50) - if attestor not registered

---

### `set_endpoint(env: Env, attestor: Address, endpoint: String)`

Set the HTTPS endpoint URL for an attestor.

**Arguments:**
- `env` - Soroban environment context
- `attestor` - Address of attestor
- `endpoint` - HTTPS URL for the attestor's API

**Returns:** None

**Errors:**
- `AttestorNotRegistered` (code 3) - if attestor not registered
- `InvalidEndpointFormat` (code 12) - if endpoint format invalid

**Side effects:** Updates attestor profile

---

### `get_endpoint(env: Env, attestor: Address) -> String`

Get the HTTPS endpoint URL for an attestor.

**Arguments:**
- `env` - Soroban environment context
- `attestor` - Address of attestor

**Returns:** Endpoint URL string

**Errors:**
- `AttestorNotRegistered` (code 3) - if attestor not registered

---

### `register_webhook(env: Env, attestor: Address, webhook_url: String)`

Register a webhook URL for an attestor.

**Arguments:**
- `env` - Soroban environment context
- `attestor` - Address of attestor
- `webhook_url` - URL where webhooks will be delivered

**Returns:** None

**Errors:**
- `AttestorNotRegistered` (code 3) - if attestor not registered

**Side effects:** Updates attestor profile with webhook URL

---

### `get_webhook_url(env: Env, attestor: Address) -> String`

Get the webhook URL for an attestor.

**Arguments:**
- `env` - Soroban environment context
- `attestor` - Address of attestor

**Returns:** Webhook URL string

**Errors:**
- `AttestorNotRegistered` (code 3) - if attestor not registered

---

## Service Configuration

### `configure_services(env: Env, anchor: Address, services: Vec<u32>)`

Configure which services an anchor supports.

**Arguments:**
- `env` - Soroban environment context
- `anchor` - Address of the anchor
- `services` - Vector of service type codes (1=deposits, 2=withdrawals, 3=quotes, 4=kyc)

**Returns:** None

**Errors:**
- `InvalidServiceType` (code 8) - if service code not recognized

**Side effects:** Stores service configuration with current schema version

---

### `configure_services_versioned(env: Env, anchor: Address, services: Vec<u32>, version: u32)`

Configure services with explicit schema version.

**Arguments:**
- `env` - Soroban environment context
- `anchor` - Address of the anchor
- `services` - Vector of service type codes
- `version` - Schema version for this configuration

**Returns:** None

**Errors:**
- `InvalidServiceType` (code 8) - if service code not recognized
- `UnsupportedCapabilityVersion` (code 29) - if version newer than current

**Side effects:** Stores versioned service configuration

---

### `current_capability_version(_env: Env) -> u32`

Get the current service capability schema version.

**Arguments:**
- `_env` - Soroban environment context

**Returns:** Current capability version (currently 1)

**Errors:** None

---

### `get_service_capability_version(env: Env, anchor: Address) -> u32`

Get the schema version of an anchor's service configuration.

**Arguments:**
- `env` - Soroban environment context
- `anchor` - Address of the anchor

**Returns:** Schema version of the anchor's service configuration

**Errors:** None

---

### `get_supported_services(env: Env, anchor: Address) -> AnchorServices`

Get all services supported by an anchor.

**Arguments:**
- `env` - Soroban environment context
- `anchor` - Address of the anchor

**Returns:** [`AnchorServices`] with service codes and version

**Errors:** None

---

### `supports_service(env: Env, anchor: Address, service: u32) -> bool`

Check if an anchor supports a specific service.

**Arguments:**
- `env` - Soroban environment context
- `anchor` - Address of the anchor
- `service` - Service type code to check

**Returns:** `true` if service supported, `false` otherwise

**Errors:** None

---

## Attestations

### `submit_attestation(env: Env, issuer: Address, subject: Address, payload_hash: Bytes, signature: Bytes) -> u64`

Submit an attestation with payload hash and signature.

**Arguments:**
- `env` - Soroban environment context
- `issuer` - Address of the attestor issuing the attestation
- `subject` - Address being attested to
- `payload_hash` - SHA-256 hash of the attested payload
- `signature` - Ed25519 signature over the payload hash

**Returns:** Attestation ID (u64)

**Errors:**
- `AttestorNotRegistered` (code 3) - if issuer not registered
- `InvalidTimestamp` (code 5) - if timestamp invalid
- `ReplayAttack` (code 6) - if duplicate attestation detected

**Side effects:** Stores attestation record, emits AttestEvent

---

### `get_attestation(env: Env, id: u64) -> Attestation`

Retrieve an attestation by ID.

**Arguments:**
- `env` - Soroban environment context
- `id` - Attestation ID

**Returns:** [`Attestation`] struct with issuer, subject, payload_hash, signature

**Errors:**
- `AttestationNotFound` (code 17) - if attestation not found

---

### `get_attestation_by_hash(env: Env, issuer: Address, payload_hash: Bytes) -> u64`

Look up an attestation ID by issuer and payload hash.

**Arguments:**
- `env` - Soroban environment context
- `issuer` - Address of the attestor
- `payload_hash` - SHA-256 hash of the payload

**Returns:** Attestation ID

**Errors:**
- `AttestationNotFound` (code 17) - if no matching attestation found

---

### `compute_payload_hash(env: Env, payload: Bytes) -> BytesN<32>`

Compute the SHA-256 hash of a payload.

**Arguments:**
- `env` - Soroban environment context
- `payload` - Raw bytes to hash

**Returns:** 32-byte SHA-256 hash

**Errors:** None

---

### `verify_payload_hash(env: Env, attestation_id: u64, expected_hash: BytesN<32>) -> bool`

Verify that an attestation's payload hash matches expected value.

**Arguments:**
- `env` - Soroban environment context
- `attestation_id` - ID of the attestation
- `expected_hash` - Expected SHA-256 hash

**Returns:** `true` if hashes match, `false` otherwise

**Errors:**
- `AttestationNotFound` (code 17) - if attestation not found

---

## KYC Management

### `submit_kyc(env: Env, subject: Address, data_hash: Bytes, attestor: Address)`

Submit KYC data for verification.

**Arguments:**
- `env` - Soroban environment context
- `subject` - Address undergoing KYC
- `data_hash` - Hash of KYC data
- `attestor` - Address of KYC attestor

**Returns:** None

**Errors:**
- `AttestorNotRegistered` (code 3) - if attestor not registered
- `RateLimitExceeded` (code 16) - if rate limit exceeded

**Side effects:** Creates KYC record with Pending status

---

### `approve_kyc(env: Env, subject: Address)`

Approve a KYC submission.

**Arguments:**
- `env` - Soroban environment context
- `subject` - Address whose KYC to approve

**Returns:** None

**Errors:**
- `KycNotFound` (code 19) - if no KYC record exists
- `UnauthorizedAttestor` (code 4) - if caller not authorized

**Side effects:** Updates KYC status to Approved

---

### `reject_kyc(env: Env, subject: Address, reason_hash: Bytes)`

Reject a KYC submission with reason.

**Arguments:**
- `env` - Soroban environment context
- `subject` - Address whose KYC to reject
- `reason_hash` - Hash of rejection reason

**Returns:** None

**Errors:**
- `KycNotFound` (code 19) - if no KYC record exists
- `UnauthorizedAttestor` (code 4) - if caller not authorized

**Side effects:** Updates KYC status to Rejected

---

### `get_kyc_status(env: Env, subject: Address) -> KycStatus`

Get the KYC status for an address.

**Arguments:**
- `env` - Soroban environment context
- `subject` - Address to check

**Returns:** [`KycStatus`] enum (NotSubmitted, Pending, Approved, Rejected, Expired)

**Errors:** None (returns NotSubmitted if no record)

---

## Sessions

### Session Lifecycle State Machine

Every session follows a formally defined lifecycle. Invalid transitions are rejected with `IllegalTransition` (code 24), `SessionClosed` (code 26), `SessionExpired` (code 25), or `SessionOperationLimitExceeded` (code 30).

```
[Created] ──(first operation)──► [Active] ──(limit reached)──► [Exhausted]
    │                                │                               │
    └──(close_session)──► [Closed] ◄─┘◄──────────────────────────── ┘
    │
    └──(ttl elapsed, any state)──► [Expired]
```

| State | Value | Description |
|-------|-------|-------------|
| `Created` | 0 | Session opened, no operations recorded yet |
| `Active` | 1 | At least one operation recorded; more allowed |
| `Exhausted` | 2 | Operation limit reached; no more operations permitted |
| `Closed` | 3 | Explicitly closed by the initiator |
| `Expired` | 4 | TTL elapsed before explicit closure |

Terminal states (`Closed`, `Expired`, `Exhausted`) accept no further transitions. `Expired` is computed at read time — if the TTL has elapsed and the stored state is non-terminal, `get_session_state` returns `4` without writing to storage.

---

### `create_session(env: Env, initiator: Address) -> u64`

Create a new session for multi-operation workflows.

**Arguments:**
- `env` - Soroban environment context
- `initiator` - Address initiating the session

**Returns:** Session ID (u64)

**Errors:**
- `NotInitialized` (code 23) - if contract not initialized

**Side effects:** Creates session record, emits SessionCreatedEvent

---

### `close_session(env: Env, session_id: u64, initiator: Address)`

Close an active session.

**Authorization:** Only the address that originally created the session (`initiator` must match the stored initiator).

**Arguments:**
- `env` - Soroban environment context
- `session_id` - ID of session to close
- `initiator` - Address that initiated the session (must authorize and match stored initiator)

**Returns:** None

**Errors:**
- `SessionNotFound` (code 32) - if no session with that ID exists
- `UnauthorizedAttestor` (code 4) - if `initiator` does not match the session's stored initiator
- `SessionExpired` (code 25) - if the session TTL has elapsed (records `Expired` state before panicking)
- `SessionClosed` (code 26) - if the session is already in `Closed` state
- `IllegalTransition` (code 24) - if the current state does not permit closing

**Side effects:** Transitions session state to `Closed`; emits `SessionClosedEvent`

---

### `get_session(env: Env, session_id: u64) -> Session`

Retrieve session details.

**Arguments:**
- `env` - Soroban environment context
- `session_id` - ID of session

**Returns:** [`Session`] struct with metadata and status

**Errors:**
- `SessionNotFound` (code 32) - if no session with that ID exists

---

### `get_session_state(env: Env, session_id: u64) -> u32`

Return the current lifecycle state of a session as a `u32` discriminant.

**State values:** `0`=Created, `1`=Active, `2`=Exhausted, `3`=Closed, `4`=Expired

This is a lightweight read. If the session's TTL has elapsed and its state is still non-terminal, `4` (Expired) is returned without writing to storage.

**Arguments:**
- `env` - Soroban environment context
- `session_id` - ID of session

**Returns:** `u32` discriminant of `SessionState`

**Errors:**
- `SessionNotFound` (code 32) - if no session with that ID exists

---

### `get_session_operation_count(env: Env, session_id: u64) -> u64`

Get the number of operations performed in a session.

**Arguments:**
- `env` - Soroban environment context
- `session_id` - ID of session

**Returns:** Operation count

**Errors:** None

---

## Quotes

### `submit_quote(env: Env, anchor: Address, base_asset: String, quote_asset: String, rate: u64, fee_percentage: u32, minimum_amount: u64, maximum_amount: u64, valid_until: u64) -> u64`

Submit a quote for an asset pair.

**Arguments:**
- `env` - Soroban environment context
- `anchor` - Address of the anchor providing the quote
- `base_asset` - Source asset code
- `quote_asset` - Destination asset code
- `rate` - Exchange rate (scaled)
- `fee_percentage` - Fee as basis points (0-10000)
- `minimum_amount` - Minimum transaction amount
- `maximum_amount` - Maximum transaction amount
- `valid_until` - Ledger timestamp when quote expires

**Returns:** Quote ID (u64)

**Errors:**
- `InvalidQuote` (code 7) - if fee > 100% or amount limits invalid
- `InvalidAssetCode` (code 53) - if asset codes invalid

**Side effects:** Stores quote record, emits QuoteSubmitEvent

---

### `receive_quote(env: Env, receiver: Address, anchor: Address, quote_id: u64) -> Quote`

Receive/retrieve a quote.

**Arguments:**
- `env` - Soroban environment context
- `receiver` - Address receiving the quote
- `anchor` - Address of the anchor
- `quote_id` - ID of the quote

**Returns:** [`Quote`] struct with all quote details

**Errors:**
- `StaleQuote` (code 10) - if quote has expired

**Side effects:** Emits QuoteReceivedEvent

---

### `get_quote(env: Env, anchor: Address, quote_id: u64) -> Quote`

Get a quote by ID.

**Arguments:**
- `env` - Soroban environment context
- `anchor` - Address of the anchor
- `quote_id` - ID of the quote

**Returns:** [`Quote`] struct

**Errors:** None

---

## Metadata Caching

### `cache_metadata(env: Env, anchor: Address, metadata: AnchorMetadata, ttl_seconds: u64)`

Cache anchor metadata with a TTL.

**Arguments:**
- `env` - Soroban environment context
- `anchor` - Address of the anchor
- `metadata` - [`AnchorMetadata`] to cache
- `ttl_seconds` - Time-to-live in seconds (0 = use default)

**Returns:** None

**Errors:** None

**Side effects:** Stores metadata in persistent cache

---

### `get_cached_metadata(env: Env, anchor: Address) -> AnchorMetadata`

Retrieve cached anchor metadata.

**Arguments:**
- `env` - Soroban environment context
- `anchor` - Address of the anchor

**Returns:** [`AnchorMetadata`]

**Errors:**
- `CacheNotFound` (code 49) - if no cached entry

---

### `get_metadata_cache_state(env: Env, anchor: Address) -> MetadataCacheState`

Get the freshness state of cached metadata without panicking.

**Arguments:**
- `env` - Soroban environment context
- `anchor` - Address of the anchor

**Returns:** [`MetadataCacheState`] (Missing, Fresh, Stale, or Expired)

**Errors:** None

---

### `refresh_metadata_cache(env: Env, anchor: Address)`

Refresh cached metadata (trigger external fetch).

**Arguments:**
- `env` - Soroban environment context
- `anchor` - Address of the anchor

**Returns:** None

**Errors:** None

**Side effects:** Records refresh diagnostic

---

## Tracing & Request Context

### `get_tracing_span(env: Env, request_id_bytes: Bytes) -> Option<TracingSpan>`

Retrieve a tracing span by request ID.

**Arguments:**
- `env` - Soroban environment context
- `request_id_bytes` - Raw bytes of the request ID

**Returns:** [`TracingSpan`] if found, None otherwise

**Errors:** None

---

### `get_trace(env: Env, root_request_id_bytes: Bytes) -> Vec<TracingSpan>`

Get all spans in a trace.

**Arguments:**
- `env` - Soroban environment context
- `root_request_id_bytes` - Root request ID bytes

**Returns:** Vector of [`TracingSpan`] ordered by span_index

**Errors:** None

---

### `create_request_context(env: Env, root_request_id: RequestId) -> RequestContext`

Create a request context for tracking operations.

**Arguments:**
- `env` - Soroban environment context
- `root_request_id` - Root [`RequestId`]

**Returns:** [`RequestContext`] with empty operation chain

**Errors:** None

**Side effects:** Stores request context

---

### `get_request_context(env: Env, root_request_id_bytes: Bytes) -> Option<RequestContext>`

Retrieve a request context.

**Arguments:**
- `env` - Soroban environment context
- `root_request_id_bytes` - Root request ID bytes

**Returns:** [`RequestContext`] if found, None otherwise

**Errors:** None

---

## Routing

### Corridor Matching Rules

Multi-asset routing selects a quote for a request by matching the request's corridor (source and destination assets) and amount against each candidate quote, then applying the requested strategy and any policy filters. The fields below are the exact names used by the routing request (`AssetPairRequest`) and candidate quote (`CandidateQuote`) structures.

**Request fields (`AssetPairRequest`):**

| Field | Type | Meaning |
|-------|------|---------|
| `base_asset` | `String` | Source asset code the caller wants to send |
| `quote_asset` | `String` | Destination asset code the caller wants to receive |
| `amount` | `u64` | Amount to route |
| `strategy` | `RoutingStrategy` | Selection strategy (e.g. `LowestFee`) |
| `min_reputation` | `u32` | Minimum reputation score a candidate anchor must have |

**Quote fields (`CandidateQuote`):**

| Field | Type | Meaning |
|-------|------|---------|
| `anchor` | `Address` | Anchor that produced the quote |
| `base_asset` | `String` | Source asset code of the quote |
| `quote_asset` | `String` | Destination asset code of the quote |
| `rate` | `u64` | Exchange rate (scaled) |
| `fee_percentage` | `u32` | Fee in basis points |
| `minimum_amount` | `u64` | Smallest amount the quote accepts |
| `maximum_amount` | `u64` | Largest amount the quote accepts |
| `valid_until` | `u64` | Ledger timestamp after which the quote is stale |
| `reputation_score` | `u32` | Anchor reputation score |
| `average_settlement_time` | `u64` | Average settlement time reported for the anchor |

A quote is eligible for a request only when **all** of the following hold:

1. `quote.base_asset` matches `request.base_asset` and `quote.quote_asset` matches `request.quote_asset` (the corridor).
2. `quote.minimum_amount <= request.amount <= quote.maximum_amount`.
3. `quote.valid_until` is in the future relative to the current ledger timestamp.
4. `quote.reputation_score >= request.min_reputation`.

Asset codes are compared **case-insensitively after normalization**, so `"xlm"` and `"XLM"` refer to the same asset. The strategy (for example `LowestFee`) only orders eligible quotes; it never makes an ineligible quote eligible. These rules describe existing matching behavior only — no new routing policy is implied.

#### Worked example: matching quote

Request:

```
AssetPairRequest {
    base_asset: "XLM",
    quote_asset: "USDC",
    amount: 100,
    strategy: RoutingStrategy::LowestFee,
    min_reputation: 50,
}
```

Candidate quote that matches:

```
CandidateQuote {
    anchor: <anchor address>,
    base_asset: "XLM",
    quote_asset: "USDC",
    rate: 10_000_000,
    fee_percentage: 25,
    minimum_amount: 10,
    maximum_amount: 1_000,
    valid_until: <future ledger timestamp>,
    reputation_score: 80,
    average_settlement_time: 30,
}
```

This quote is eligible: the corridor matches (`XLM` → `USDC`), `minimum_amount` (10) <= `amount` (100) <= `maximum_amount` (1_000), `valid_until` is in the future, and `reputation_score` (80) >= `min_reputation` (50).

#### Worked example: corridor mismatch

A second candidate quote for the same request:

```
CandidateQuote {
    anchor: <anchor address>,
    base_asset: "XLM",
    quote_asset: "EUR",
    rate: 9_500_000,
    fee_percentage: 10,
    minimum_amount: 10,
    maximum_amount: 1_000,
    valid_until: <future ledger timestamp>,
    reputation_score: 90,
    average_settlement_time: 20,
}
```

This quote is **ineligible** even though its fee is lower and its reputation higher: its `quote_asset` is `EUR`, which does not match the request's `quote_asset` of `USDC`. The corridor mismatch alone disqualifies it, and `LowestFee` cannot select it.

---

### `route_transaction(env: Env, options: RoutingOptions) -> Quote`

Route a transaction to the best anchor based on strategy.

**Arguments:**
- `env` - Soroban environment context
- `options` - [`RoutingOptions`] with request, strategy, filters

**Returns:** [`Quote`] from the selected anchor

**Errors:**
- `NoQuotesAvailable` (code 13) - if no suitable anchors found
- `InvalidWeights` (code 31) - if routing weights invalid

---

### `route_anchors(env: Env, options: RoutingOptions) -> Vec<Address>`

Get a ranked list of anchors for a transaction.

**Arguments:**
- `env` - Soroban environment context
- `options` - [`RoutingOptions`] with request, strategy, filters

**Returns:** Vector of [`Address`] ranked by score

**Errors:**
- `InvalidWeights` (code 31) - if routing weights invalid

---

## Anchor Info Discovery

### `fetch_anchor_info(env: Env, anchor: Address, ttl_seconds: u64) -> StellarToml`

Fetch and cache anchor's stellar.toml.

**Arguments:**
- `env` - Soroban environment context
- `anchor` - Address of the anchor
- `ttl_seconds` - Cache TTL (0 = use default)

**Returns:** [`StellarToml`] with anchor capabilities and assets

**Errors:**
- `InvalidEndpointFormat` (code 12) - if endpoint URL invalid

**Side effects:** Caches stellar.toml

---

### `get_anchor_toml(env: Env, anchor: Address) -> StellarToml`

Get cached stellar.toml for an anchor.

**Arguments:**
- `env` - Soroban environment context
- `anchor` - Address of the anchor

**Returns:** [`StellarToml`]

**Errors:**
- `CacheNotFound` (code 49) - if not cached

---

### `get_anchor_assets(env: Env, anchor: Address) -> Vec<String>`

Get list of asset codes supported by an anchor.

**Arguments:**
- `env` - Soroban environment context
- `anchor` - Address of the anchor

**Returns:** Vector of asset codes

**Errors:** None

---

### `get_anchor_asset_info(env: Env, anchor: Address, asset_code: String) -> AssetInfo`

Get detailed info for a specific asset.

**Arguments:**
- `env` - Soroban environment context
- `anchor` - Address of the anchor
- `asset_code` - Asset code to look up

**Returns:** [`AssetInfo`] with fees, limits, enabled status

**Errors:** None

---

## Rate Limiting

### `set_rate_limit_config(env: Env, caller: Address, max_submissions: u32, window_length: u32)`

Configure the global rate limit for attestation submissions.

**Authorization:** Primary admin or holder of `AdminCapability::SetRateLimits`.

**Arguments:**
- `env` - Soroban environment context
- `caller` - Address authorizing the change (must be admin or have `SetRateLimits` capability)
- `max_submissions` - Maximum submissions per window
- `window_length` - Window length in ledgers

**Returns:** None

**Errors:**
- `Unauthorized` (code 28) - if caller lacks permission
- `ValidationError` (code 15) - if config values are invalid

**Side effects:** Updates rate limiter configuration; writes audit log entry

---

### `set_role_rate_limit(env: Env, caller: Address, role: Symbol, config: RateLimitConfig)`

Set a per-role rate limit override.

**Authorization:** Primary admin or holder of `AdminCapability::SetRateLimits`.

**Arguments:**
- `env` - Soroban environment context
- `caller` - Authorizing address
- `role` - Role symbol to apply the override to
- `config` - `RateLimitConfig` with max_submissions and window_length

**Returns:** None

**Errors:**
- `Unauthorized` (code 28) - if caller lacks permission
- `ValidationError` (code 15) - if config is invalid

---

## Role-Based Access Control

### `grant_role(env: Env, grantee: Address, role: AdminRole)`

Grant a delegated admin role to an address. Only the primary admin may call this.

**Arguments:**
- `env` - Soroban environment context
- `grantee` - Address to grant the role to
- `role` - `AdminRole` variant (`KycAdmin`, `AttestorAdmin`, or `CacheAdmin`)

**Returns:** None

**Errors:**
- `NotInitialized` (code 23) - if contract not initialized
- `Unauthorized` (code 28) - if caller is not the primary admin

**Side effects:** Writes role grant to persistent storage; writes audit log entry; emits `role.granted` event

---

### `revoke_role(env: Env, grantee: Address, role: AdminRole)`

Revoke a delegated admin role. Only the primary admin may call this. Revoking a role never held is a no-op.

**Arguments:**
- `env` - Soroban environment context
- `grantee` - Address to revoke the role from
- `role` - `AdminRole` variant to revoke

**Returns:** None

**Errors:**
- `Unauthorized` (code 28) - if caller is not the primary admin

**Side effects:** Removes role grant from persistent storage; writes audit log entry; emits `role.revoked` event

---

### `has_role(env: Env, address: Address, role: AdminRole) -> bool`

Check whether an address holds a role (or is the primary admin).

**Arguments:**
- `env` - Soroban environment context
- `address` - Address to check
- `role` - `AdminRole` to check for

**Returns:** `true` if the address holds the role or is the primary admin

---

## Fine-Grained Capability Management

### `grant_capability(env: Env, grantee: Address, capability: AdminCapability)`

Grant a fine-grained capability to an address. Only the primary admin may call this. Granting a capability already held is idempotent.

**Arguments:**
- `env` - Soroban environment context
- `grantee` - Address to grant the capability to
- `capability` - `AdminCapability` variant to grant

**Returns:** None

**Errors:**
- `NotInitialized` (code 23) - if contract not initialized
- `Unauthorized` (code 28) - if caller is not the primary admin

**Side effects:** Writes capability grant to persistent storage; writes audit log entry; emits `cap.granted` event

---

### `revoke_capability(env: Env, grantee: Address, capability: AdminCapability)`

Revoke a fine-grained capability. Only the primary admin may call this. Revoking a capability never granted is a no-op.

**Arguments:**
- `env` - Soroban environment context
- `grantee` - Address to revoke the capability from
- `capability` - `AdminCapability` variant to revoke

**Returns:** None

**Errors:**
- `Unauthorized` (code 28) - if caller is not the primary admin

**Side effects:** Removes capability grant from persistent storage; writes audit log entry; emits `cap.revoked` event

---

### `has_capability(env: Env, address: Address, capability: AdminCapability) -> bool`

Check whether an address holds a capability (or is the primary admin).

**Arguments:**
- `env` - Soroban environment context
- `address` - Address to check
- `capability` - `AdminCapability` variant to check for

**Returns:** `true` if the address holds the capability or is the primary admin

---

## Service Toggle Management

### `enable_service(env: Env, caller: Address, anchor: Address, service_code: u32) -> bool`

Enable a single service for an anchor in the toggle store.

**Authorization:** Primary admin or holder of `AdminCapability::ToggleServices`.

**Arguments:**
- `env` - Soroban environment context
- `caller` - Authorizing address
- `anchor` - Anchor address
- `service_code` - Service type code (1=deposits, 2=withdrawals, 3=quotes, 4=kyc, 5=sep31)

**Returns:** `true` if the service was changed from disabled to enabled; `false` if already enabled

**Errors:**
- `Unauthorized` (code 28) - if caller lacks permission

---

### `disable_service(env: Env, caller: Address, anchor: Address, service_code: u32) -> bool`

Disable a single service for an anchor in the toggle store.

**Authorization:** Primary admin or holder of `AdminCapability::ToggleServices`.

**Returns:** `true` if changed; `false` if already disabled

---

### `snapshot_services(env: Env, caller: Address, anchor: Address, services: Vec<u32>, description: String) -> u64`

Snapshot the current service configuration for `anchor` so it can be restored via `rollback_services`.

**Authorization:** Primary admin or holder of `AdminCapability::ToggleServices`.

**Returns:** Snapshot ID (u64)

---

### `rollback_services(env: Env, caller: Address, snapshot_id: u64) -> bool`

Restore service configuration from a previously taken snapshot.

**Authorization:** Primary admin or holder of `AdminCapability::ToggleServices`.

**Returns:** `true` if the snapshot was found and applied; `false` otherwise

---

## Error Codes Reference

| Code | Name | Description |
|------|------|-------------|
| 1 | AlreadyInitialized | Contract is already initialized |
| 2 | AttestorAlreadyRegistered | Attestor is already registered |
| 3 | AttestorNotRegistered | Attestor is not registered |
| 4 | UnauthorizedAttestor | Attestor is not authorized |
| 5 | InvalidTimestamp | Timestamp is invalid |
| 6 | ReplayAttack | Replay attack detected |
| 7 | InvalidQuote | Quote is invalid |
| 8 | InvalidServiceType | Service type is invalid |
| 9 | InvalidTransactionIntent | Transaction intent is invalid |
| 10 | StaleQuote | Quote has expired |
| 11 | ComplianceNotMet | Compliance requirements not met |
| 12 | InvalidEndpointFormat | Endpoint format is invalid |
| 13 | NoQuotesAvailable | No quotes are available |
| 14 | ServicesNotConfigured | Services are not configured |
| 15 | ValidationError | Response schema validation failed |
| 16 | RateLimitExceeded | Rate limit exceeded |
| 17 | AttestationNotFound | Attestation not found |
| 18 | InvalidSep10Token | SEP-10 JWT is missing, expired, or invalid |
| 19 | KycNotFound | KYC record not found |
| 20 | KycPending | KYC verification is pending |
| 21 | KycRejected | KYC verification was rejected |
| 22 | WebhookDeliveryFailed | Webhook delivery failed validation |
| 23 | NotInitialized | Contract is not initialized |
| 24 | IllegalTransition | Illegal transaction state transition |
| 25 | SessionExpired | Session has expired |
| 26 | SessionClosed | Session is closed |
| 27 | UnsupportedCapabilityVersion | Service capability version is unsupported; also returned when `migrate()` is called with a schema version the current binary does not know about |
| 28 | **Unauthorized** | **Caller does not hold the required admin role or capability for this operation** |
| 30 | SessionOperationLimitExceeded | Session operation limit exceeded |
| 31 | InvalidWeights | Routing weights must sum to 1.0 |
| 32 | SessionNotFound | Session not found |
| 33 | QuoteNotFound | Quote not found |
| 34 | AuditLogNotFound | Audit log not found |
| 35 | TransactionNotFound | Transaction record not found |
| 48 | CacheExpired | Cache entry has expired |
| 49 | CacheNotFound | Cache entry not found |
| 50 | AttestorProfileNotFound | Attestor profile not found |
| 51 | InvalidRequestContext | Request context is invalid |
| 52 | InvalidSessionMetadata | Session metadata is invalid |
| 53 | InvalidAssetCode | Asset code is invalid |
| 54 | AttestorCapacityExceeded | Attestor capacity has been exceeded |
| 55 | CacheCapacityExceeded | Cache capacity has been exceeded |
| 56 | EndpointNotSet | Attestor endpoint URL is not set |
| 57 | WebhookUrlNotSet | Attestor webhook URL is not set |

> **Note on authorization errors:** All privilege-check failures now consistently produce `Unauthorized` (code 28) regardless of whether the check was a role check, a capability check, or a raw admin check. The legacy `UnauthorizedAttestor` (code 4) is preserved for backwards compatibility but is only returned by attestation-submission paths, not by admin management operations.
