use usage_contracts::*;
#[test]
fn wire_preserves_missing_and_large_tokens() {
    let missing = Metric::<String> {
        value: None,
        accuracy: Accuracy::Unavailable,
        known_rows: 0,
        missing_rows: 1,
    };
    let json = serde_json::to_value(missing).unwrap();
    assert!(json["value"].is_null());
    assert_eq!(json["accuracy"], "unavailable");
    let exact = Metric {
        value: Some("9223372036854775807".to_owned()),
        accuracy: Accuracy::Exact,
        known_rows: 1,
        missing_rows: 0,
    };
    assert_eq!(
        serde_json::to_value(exact).unwrap()["value"],
        "9223372036854775807"
    );
}
#[test]
fn wire_is_camel_case_and_error_codes_are_stable() {
    let request: StartScanRequest = serde_json::from_str(
        r#"{"providerId":"ccusage.claude-code","timezone":"America/Phoenix"}"#,
    )
    .unwrap();
    assert_eq!(request.provider_id, "ccusage.claude-code");
    assert_eq!(
        serde_json::to_string(&ErrorCode::SchemaUnsupported).unwrap(),
        "\"SCHEMA_UNSUPPORTED\""
    );
    assert!(serde_json::from_str::<JobRequest>(r#"{"jobId":"x","command":"shell"}"#).is_err());
}
