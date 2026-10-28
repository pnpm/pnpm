use super::PIN_DECISION_SETTINGS;

#[test]
fn pin_decision_settings_match_shared_fixture() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../../../../fixtures/pin-decision.json"))
            .expect("fixtures/pin-decision.json is valid JSON");
    let expected: Vec<&str> = fixture["PIN_DECISION_SETTINGS"]
        .as_array()
        .expect("PIN_DECISION_SETTINGS is an array")
        .iter()
        .map(|value| value.as_str().expect("setting name is a string"))
        .collect();
    assert_eq!(PIN_DECISION_SETTINGS, expected.as_slice());
}
