#![cfg(test)]

mod webhook_middleware_tests {
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    use anchorkit::{
        errors::ErrorCode,
        retry::RetryConfig,
        webhook::{
            deliver_webhook, get_dead_letter_webhooks, verify_webhook_signature,
            verify_webhook_signature_with_replay_protection, DlqEntry,
            MemoryNonceTracker, VerificationResult, WebhookDeliveryConfig, MAX_WEBHOOK_BODY_BYTES,
        },
    };

    fn config(max_retries: u32) -> WebhookDeliveryConfig {
        WebhookDeliveryConfig {
            endpoint_url: "https://example.com/hook".into(),
            timeout_ms: 1000,
            retry_config: RetryConfig::new(max_retries, 0, 0, 1),
            dead_letter_storage_key: "test_dlq".into(),
            signing_key: None,
            max_payload_age_seconds: None,
            require_nonce_for_replay_protection: false,
        }
    }

    fn signed_config(max_retries: u32, key: Vec<u8>) -> WebhookDeliveryConfig {
        WebhookDeliveryConfig {
            endpoint_url: "https://example.com/hook".into(),
            timeout_ms: 1000,
            retry_config: RetryConfig::new(max_retries, 0, 0, 1),
            dead_letter_storage_key: "test_dlq".into(),
            signing_key: Some(key),
            max_payload_age_seconds: None,
            require_nonce_for_replay_protection: false,
        }
    }

    // -----------------------------------------------------------------------
    // 1. Immediate success — no retries triggered
    // -----------------------------------------------------------------------
    #[test]
    fn test_immediate_success_no_retries() {
        let call_count = Arc::new(Mutex::new(0u32));
        let cc = call_count.clone();

        let mut dlq: BTreeMap<String, Vec<DlqEntry>> = BTreeMap::new();
        let result = deliver_webhook(
            &config(3),
            r#"{"event":"deposit"}"#,
            &mut dlq,
            move |_url, _body, _sig| {
                *cc.lock().unwrap() += 1;
                Ok(200)
            },
            |_| {},
            || 1_000_000u64,
        );

        assert!(result.is_ok());
        assert_eq!(*call_count.lock().unwrap(), 1, "should call HTTP exactly once");
        assert!(dlq.is_empty(), "DLQ must be empty on success");
    }

    // -----------------------------------------------------------------------
    // 2. Two 503s then 200 — succeeds on 3rd attempt
    // -----------------------------------------------------------------------
    #[test]
    fn test_success_after_two_failures() {
        let call_count = Arc::new(Mutex::new(0u32));
        let cc = call_count.clone();

        let mut dlq: BTreeMap<String, Vec<DlqEntry>> = BTreeMap::new();
        let result = deliver_webhook(
            &config(3),
            r#"{"event":"withdrawal"}"#,
            &mut dlq,
            move |_url, _body, _sig| {
                let mut n = cc.lock().unwrap();
                *n += 1;
                if *n < 3 { Ok(503) } else { Ok(200) }
            },
            |_| {},
            || 1_000_000u64,
        );

        assert!(result.is_ok(), "expected success on 3rd attempt, got {:?}", result);
        assert_eq!(*call_count.lock().unwrap(), 3);
        assert!(dlq.is_empty(), "DLQ must be empty when delivery eventually succeeds");
    }

    // -----------------------------------------------------------------------
    // 3. All retries exhausted — payload lands in DLQ
    // -----------------------------------------------------------------------
    #[test]
    fn test_exhausted_retries_writes_to_dlq() {
        let payload = r#"{"event":"kyc_failed"}"#;
        let mut dlq: BTreeMap<String, Vec<DlqEntry>> = BTreeMap::new();

        let result = deliver_webhook(
            &config(3),
            payload,
            &mut dlq,
            |_url, _body, _sig| Ok(503u16),
            |_| {},
            || 1_000_000u64,
        );

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, ErrorCode::WebhookDeliveryFailed);
        let ctx = err.context.expect("context must be set");
        assert!(ctx.contains("attempts_made=3"), "context: {ctx}");

        let entries = get_dead_letter_webhooks(&dlq, "test_dlq");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].payload, payload);
        assert_eq!(entries[0].attempts_made, 3);
        assert_eq!(entries[0].last_status_code, 503);
    }

    // -----------------------------------------------------------------------
    // 4. Admin inspection — get_dead_letter_webhooks returns all failed payloads
    // -----------------------------------------------------------------------
    #[test]
    fn test_admin_can_inspect_dlq() {
        let mut dlq: BTreeMap<String, Vec<DlqEntry>> = BTreeMap::new();
        let payloads = [r#"{"event":"a"}"#, r#"{"event":"b"}"#];

        for p in &payloads {
            let _ = deliver_webhook(
                &config(1),
                p,
                &mut dlq,
                |_url, _body, _sig| Ok(500u16),
                |_| {},
                || 1_000_000u64,
            );
        }

        let entries = get_dead_letter_webhooks(&dlq, "test_dlq");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].payload, payloads[0]);
        assert_eq!(entries[1].payload, payloads[1]);
        assert!(get_dead_letter_webhooks(&dlq, "no_such_key").is_empty());
    }

    // -----------------------------------------------------------------------
    // 5. Signed payload produces X-Anchor-Signature header
    // -----------------------------------------------------------------------
    #[test]
    fn test_signed_payload_produces_header() {
        let key = b"my-secret-key".to_vec();
        let payload = r#"{"event":"deposit"}"#;
        let received_sig: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let rs = received_sig.clone();

        let mut dlq: BTreeMap<String, Vec<DlqEntry>> = BTreeMap::new();
        let result = deliver_webhook(
            &signed_config(1, key.clone()),
            payload,
            &mut dlq,
            move |_url, _body, sig| {
                *rs.lock().unwrap() = sig.map(|s| s.to_string());
                Ok(200)
            },
            |_| {},
            || 0u64,
        );
        assert!(result.is_ok());
        let sig = received_sig.lock().unwrap().clone().expect("signature must be set");
        assert!(sig.starts_with("sha256="), "got: {sig}");
        // Verify the signature is valid
        assert!(verify_webhook_signature(payload, &sig, &key), "signature should verify");
    }

    // -----------------------------------------------------------------------
    // 6. Unsigned config produces no header
    // -----------------------------------------------------------------------
    #[test]
    fn test_unsigned_config_produces_no_header() {
        let received_sig: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let rs = received_sig.clone();
        let mut dlq: BTreeMap<String, Vec<DlqEntry>> = BTreeMap::new();
        let _ = deliver_webhook(
            &config(1),
            "payload",
            &mut dlq,
            move |_url, _body, sig| {
                *rs.lock().unwrap() = sig.map(|s| s.to_string());
                Ok(200)
            },
            |_| {},
            || 0u64,
        );
        assert!(received_sig.lock().unwrap().is_none(), "unsigned config must not produce a header");
    }

    // -----------------------------------------------------------------------
    // 7. verify_webhook_signature — correct key returns true
    // -----------------------------------------------------------------------
    #[test]
    fn test_verify_correct_signature() {
        let key = b"secret";
        let payload = r#"{"event":"test"}"#;
        // Build header using sign_payload indirectly via deliver_webhook
        let sig_captured: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let sc = sig_captured.clone();
        let mut dlq: BTreeMap<String, Vec<DlqEntry>> = BTreeMap::new();
        let _ = deliver_webhook(
            &signed_config(1, key.to_vec()),
            payload,
            &mut dlq,
            move |_url, _body, sig| {
                if let Some(s) = sig { *sc.lock().unwrap() = s.to_string(); }
                Ok(200)
            },
            |_| {},
            || 0u64,
        );
        let sig = sig_captured.lock().unwrap().clone();
        assert!(verify_webhook_signature(payload, &sig, key));
    }

    // -----------------------------------------------------------------------
    // 8. verify_webhook_signature — tampered payload returns false
    // -----------------------------------------------------------------------
    #[test]
    fn test_verify_tampered_payload_fails() {
        let key = b"secret";
        let payload = r#"{"event":"test"}"#;
        let sig_captured: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let sc = sig_captured.clone();
        let mut dlq: BTreeMap<String, Vec<DlqEntry>> = BTreeMap::new();
        let _ = deliver_webhook(
            &signed_config(1, key.to_vec()),
            payload,
            &mut dlq,
            move |_url, _body, sig| {
                if let Some(s) = sig { *sc.lock().unwrap() = s.to_string(); }
                Ok(200)
            },
            |_| {},
            || 0u64,
        );
        let sig = sig_captured.lock().unwrap().clone();
        assert!(!verify_webhook_signature(r#"{"event":"tampered"}"#, &sig, key));
    }

    // -----------------------------------------------------------------------
    // 9. verify_webhook_signature — body limit is checked before HMAC
    // -----------------------------------------------------------------------
    #[test]
    fn test_verify_rejects_oversized_body_but_accepts_limit() {
        let key = b"secret";
        let at_limit = "a".repeat(MAX_WEBHOOK_BODY_BYTES);
        let oversized = "a".repeat(MAX_WEBHOOK_BODY_BYTES + 1);
        let signature = Arc::new(Mutex::new(String::new()));
        let captured = signature.clone();
        let mut dlq: BTreeMap<String, Vec<DlqEntry>> = BTreeMap::new();
        let result = deliver_webhook(
            &signed_config(1, key.to_vec()),
            &at_limit,
            &mut dlq,
            move |_url, _body, sig| {
                if let Some(sig) = sig {
                    *captured.lock().unwrap() = sig.to_string();
                }
                Ok(200)
            },
            |_| {},
            || 0,
        );
        assert!(result.is_ok());
        let signature = signature.lock().unwrap().clone();
        assert!(verify_webhook_signature(&at_limit, &signature, key));
        assert!(!verify_webhook_signature(&oversized, &signature, key));
    }

    // -----------------------------------------------------------------------
    // 10. verify_webhook_signature — wrong key returns false
    // -----------------------------------------------------------------------
    #[test]
    fn test_verify_wrong_key_fails() {
        let key = b"secret";
        let payload = r#"{"event":"test"}"#;
        let sig_captured: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let sc = sig_captured.clone();
        let mut dlq: BTreeMap<String, Vec<DlqEntry>> = BTreeMap::new();
        let _ = deliver_webhook(
            &signed_config(1, key.to_vec()),
            payload,
            &mut dlq,
            move |_url, _body, sig| {
                if let Some(s) = sig { *sc.lock().unwrap() = s.to_string(); }
                Ok(200)
            },
            |_| {},
            || 0u64,
        );
        let sig = sig_captured.lock().unwrap().clone();
        assert!(!verify_webhook_signature(payload, &sig, b"wrong-key"));
    }

    // -----------------------------------------------------------------------
    // 10. verify_webhook_signature — empty secret is rejected before HMAC work
    // -----------------------------------------------------------------------
    #[test]
    fn test_verify_empty_secret_fails() {
        let key = b"secret";
        let payload = r#"{"event":"test"}"#;
        let sig_captured: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let sc = sig_captured.clone();
        let mut dlq: BTreeMap<String, Vec<DlqEntry>> = BTreeMap::new();
        let _ = deliver_webhook(
            &signed_config(1, key.to_vec()),
            payload,
            &mut dlq,
            move |_url, _body, sig| {
                if let Some(s) = sig { *sc.lock().unwrap() = s.to_string(); }
                Ok(200)
            },
            |_| {},
            || 0u64,
        );
        let sig = sig_captured.lock().unwrap().clone();

        // A blank secret must never validate a signature, even one produced
        // with a real key.
        assert!(!verify_webhook_signature(payload, &sig, b""));
    }

    // -----------------------------------------------------------------------
    // 11. max_age_seconds == 0 is treated as "unlimited" (no age check)
    // -----------------------------------------------------------------------

    /// max_age_seconds = 0 must not silently reject every message.
    /// Zero is defined as "unlimited" (skip the age check).
    #[test]
    fn test_max_age_zero_means_unlimited() {
        // Capture the signature for a payload with a timestamp far in the past.
        let payload = r#"{"timestamp":1,"nonce":"old-but-valid"}"#;
        let key = b"secret";
        let sig_captured: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let sc = sig_captured.clone();
        let mut dlq: BTreeMap<String, Vec<DlqEntry>> = BTreeMap::new();
        let _ = deliver_webhook(
            &signed_config(1, key.to_vec()),
            payload,
            &mut dlq,
            move |_url, _body, sig| {
                if let Some(s) = sig { *sc.lock().unwrap() = s.to_string(); }
                Ok(200)
            },
            |_| {},
            || 0u64,
        );
        let sig = sig_captured.lock().unwrap().clone();

        let mut tracker = MemoryNonceTracker::new();
        // current_time = 1_000_000, timestamp = 1 → age = 999_999 seconds
        // With max_age_seconds = 0, this must succeed (unlimited).
        assert_eq!(
            verify_webhook_signature_with_replay_protection(
                payload, &sig, key, 1_000_000, 0, &mut tracker,
            ),
            VerificationResult::Valid,
            "max_age_seconds=0 must behave as unlimited, not reject every message"
        );
    }

    /// A positive max_age_seconds still enforces the age limit correctly.
    #[test]
    fn test_positive_max_age_still_enforced() {
        let payload = r#"{"timestamp":900,"nonce":"aged-out"}"#;
        let key = b"secret";
        let sig_captured: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let sc = sig_captured.clone();
        let mut dlq: BTreeMap<String, Vec<DlqEntry>> = BTreeMap::new();
        let _ = deliver_webhook(
            &signed_config(1, key.to_vec()),
            payload,
            &mut dlq,
            move |_url, _body, sig| {
                if let Some(s) = sig { *sc.lock().unwrap() = s.to_string(); }
                Ok(200)
            },
            |_| {},
            || 0u64,
        );
        let sig = sig_captured.lock().unwrap().clone();

        let mut tracker = MemoryNonceTracker::new();
        // age = 1000 - 900 = 100, limit = 99 → should be expired
        assert_eq!(
            verify_webhook_signature_with_replay_protection(
                payload, &sig, key, 1000, 99, &mut tracker,
            ),
            VerificationResult::InvalidTimestamp,
            "message older than max_age_seconds must be rejected"
        );
    }

    // -----------------------------------------------------------------------
    // 12. Empty nonce is rejected before tracker mutation
    // -----------------------------------------------------------------------

    /// An empty nonce must fail before the nonce tracker is consulted or mutated.
    #[test]
    fn test_empty_nonce_rejected_before_tracker_mutation() {
        let payload = r#"{"timestamp":1000,"nonce":""}"#;
        let key = b"secret";
        let sig_captured: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let sc = sig_captured.clone();
        let mut dlq: BTreeMap<String, Vec<DlqEntry>> = BTreeMap::new();
        let _ = deliver_webhook(
            &signed_config(1, key.to_vec()),
            payload,
            &mut dlq,
            move |_url, _body, sig| {
                if let Some(s) = sig { *sc.lock().unwrap() = s.to_string(); }
                Ok(200)
            },
            |_| {},
            || 0u64,
        );
        let sig = sig_captured.lock().unwrap().clone();

        let mut tracker = MemoryNonceTracker::new();
        let result = verify_webhook_signature_with_replay_protection(
            payload, &sig, key, 1000, 0, &mut tracker,
        );
        assert_eq!(
            result,
            VerificationResult::InvalidSignature,
            "empty nonce must be rejected as InvalidSignature before tracker mutation"
        );
    }

    /// Two signed payloads with an empty nonce both fail independently,
    /// proving the tracker was never mutated by the first call.
    #[test]
    fn test_two_empty_nonce_payloads_both_fail() {
        let payload = r#"{"timestamp":1000,"nonce":""}"#;
        let key = b"secret";
        let sig_captured: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let sc = sig_captured.clone();
        let mut dlq: BTreeMap<String, Vec<DlqEntry>> = BTreeMap::new();
        let _ = deliver_webhook(
            &signed_config(1, key.to_vec()),
            payload,
            &mut dlq,
            move |_url, _body, sig| {
                if let Some(s) = sig { *sc.lock().unwrap() = s.to_string(); }
                Ok(200)
            },
            |_| {},
            || 0u64,
        );
        let sig = sig_captured.lock().unwrap().clone();

        let mut tracker = MemoryNonceTracker::new();
        for i in 0..2 {
            assert_eq!(
                verify_webhook_signature_with_replay_protection(
                    payload, &sig, key, 1000, 0, &mut tracker,
                ),
                VerificationResult::InvalidSignature,
                "empty-nonce call #{} must fail as InvalidSignature", i
            );
        }
    }

    // -----------------------------------------------------------------------
    // Issue #1286 — maximum delivery attempts with always-failing transport
    // -----------------------------------------------------------------------

    /// A transport that always returns a connection error must be called exactly
    /// max_attempts times, the result must be WebhookDeliveryFailed, and the DLQ
    /// entry must record attempts_made == max_attempts with status 0 (transport err).
    #[test]
    fn test_always_failing_transport_exhausts_configured_attempts() {
        let call_count = Arc::new(Mutex::new(0u32));
        let cc = call_count.clone();
        let max_attempts = 4u32;

        let mut dlq: BTreeMap<String, Vec<DlqEntry>> = BTreeMap::new();
        let result = deliver_webhook(
            &config(max_attempts),
            r#"{"event":"ping"}"#,
            &mut dlq,
            move |_url, _body, _sig| {
                *cc.lock().unwrap() += 1;
                Err("connection refused".to_string())
            },
            |_| {},
            || 1_000_000u64,
        );

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, ErrorCode::WebhookDeliveryFailed);
        assert_eq!(
            *call_count.lock().unwrap(),
            max_attempts,
            "transport must be called exactly max_attempts={} times",
            max_attempts,
        );
        let entries = get_dead_letter_webhooks(&dlq, "test_dlq");
        assert_eq!(entries.len(), 1, "exactly one DLQ entry must be written");
        assert_eq!(entries[0].attempts_made, max_attempts);
        assert_eq!(entries[0].last_status_code, 0, "transport failure sets status to 0");
    }

    // -----------------------------------------------------------------------
    // Issue #1287 — a success after a failure must reset failure state
    // -----------------------------------------------------------------------

    /// A failed delivery followed by a successful delivery must leave the
    /// observable state reporting success: the DLQ keeps only the failed
    /// payload, the success appends nothing, and the final attempt is not
    /// recorded as failed. The fake transport fails once then succeeds, so no
    /// real HTTP calls occur.
    #[test]
    fn test_success_after_failure_resets_failure_state() {
        let call_count = Arc::new(Mutex::new(0u32));
        let cc = call_count.clone();

        let mut dlq: BTreeMap<String, Vec<DlqEntry>> = BTreeMap::new();

        // First delivery: transport fails on every attempt -> DLQ entry.
        let failed_payload = r#"{"event":"first"}"#;
        let first = deliver_webhook(
            &config(1),
            failed_payload,
            &mut dlq,
            move |_url, _body, _sig| {
                *cc.lock().unwrap() += 1;
                Err("connection refused".to_string())
            },
            |_| {},
            || 1_000_000u64,
        );

        assert!(first.is_err(), "first delivery must fail");
        assert_eq!(
            first.unwrap_err().code,
            ErrorCode::WebhookDeliveryFailed,
            "failure must surface as WebhookDeliveryFailed",
        );
        let entries_after_failure = get_dead_letter_webhooks(&dlq, "test_dlq");
        assert_eq!(entries_after_failure.len(), 1, "failed delivery must be recorded in DLQ");
        assert_eq!(entries_after_failure[0].payload, failed_payload);
        assert_eq!(entries_after_failure[0].attempts_made, 1);
        assert_eq!(entries_after_failure[0].last_status_code, 0);

        // Second delivery: transport succeeds on the first attempt.
        let success_payload = r#"{"event":"second"}"#;
        let second = deliver_webhook(
            &config(1),
            success_payload,
            &mut dlq,
            move |_url, _body, _sig| {
                *cc.lock().unwrap() += 1;
                Ok(200)
            },
            |_| {},
            || 1_000_000u64,
        );

        assert!(second.is_ok(), "second delivery must succeed, got {:?}", second);
        assert_eq!(
            *call_count.lock().unwrap(),
            2,
            "transport must be called once per delivery",
        );

        // Success must not append to the DLQ and must not overwrite the
        // earlier failure record.
        let entries_after_success = get_dead_letter_webhooks(&dlq, "test_dlq");
        assert_eq!(
            entries_after_success.len(),
            1,
            "successful delivery must not append to the DLQ",
        );
        assert_eq!(entries_after_success[0].payload, failed_payload);
        assert!(
            entries_after_success.iter().all(|e| e.payload != success_payload),
            "successful payload must never appear in the DLQ",
        );

        // The final observable state for the successful payload is success:
        // a subsequent delivery of the same payload also succeeds and still
        // leaves the DLQ untouched, proving no stale failure state remains.
        let third = deliver_webhook(
            &config(1),
            success_payload,
            &mut dlq,
            |_url, _body, _sig| Ok(200u16),
            |_| {},
            || 1_000_000u64,
        );
        assert!(third.is_ok(), "repeated success must remain success");
        assert_eq!(
            get_dead_letter_webhooks(&dlq, "test_dlq").len(),
            1,
            "DLQ depth must stay at the single recorded failure",
        );
    }

    /// Non-empty nonces retain their normal signature, age, and replay semantics.
    #[test]
    fn test_nonempty_nonce_normal_semantics() {
        let payload = r#"{"timestamp":1000,"nonce":"unique-xyz"}"#;
        let key = b"secret";
        let sig_captured: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let sc = sig_captured.clone();
        let mut dlq: BTreeMap<String, Vec<DlqEntry>> = BTreeMap::new();
        let _ = deliver_webhook(
            &signed_config(1, key.to_vec()),
            payload,
            &mut dlq,
            move |_url, _body, sig| {
                if let Some(s) = sig { *sc.lock().unwrap() = s.to_string(); }
                Ok(200)
            },
            |_| {},
            || 0u64,
        );
        let sig = sig_captured.lock().unwrap().clone();

        let mut tracker = MemoryNonceTracker::new();
        // First call: Valid
        assert_eq!(
            verify_webhook_signature_with_replay_protection(
                payload, &sig, key, 1000, 0, &mut tracker,
            ),
            VerificationResult::Valid
        );
        // Second call: ReplayDetected
        assert_eq!(
            verify_webhook_signature_with_replay_protection(
                payload, &sig, key, 1000, 0, &mut tracker,
            ),
            VerificationResult::ReplayDetected
        );
    }
}
