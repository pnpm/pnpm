use super::PIN_DECISION_SETTINGS;

#[test]
fn pin_decision_settings_match_shared_fixture() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../../../../fixtures/pin-decision.json"))
            .expect("fixtures/pin-decision.json is valid JSON");
    let mut expected: Vec<&str> = ["PIN_DECISION_SETTINGS", "PIN_DECISION_SETTINGS_V12_ONLY"]
        .into_iter()
        .flat_map(|key| fixture[key].as_array().expect("the settings list is an array"))
        .map(serde_json::Value::as_str)
        .map(|text| text.expect("setting name is a string"))
        .collect();
    expected.sort_unstable();
    assert_eq!(PIN_DECISION_SETTINGS, expected.as_slice());
}
