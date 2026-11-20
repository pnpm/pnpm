use super::parse;
use serde_json::json;

#[test]
fn reads_json5_numbers_strings_and_comments() {
    let value = parse(
        r"// heading
        {hex: -0x10, positive: +0x10, emoji: '\uD83D\uDE00', fraction: .5,}
    ",
    )
    .unwrap();
    assert_eq!(value, json!({"hex": -16, "positive": 16, "emoji": "😀", "fraction": 0.5}));
}

#[test]
fn rejects_excessive_nesting_before_building_a_deep_value() {
    for (opening, closing) in [("[", "]"), ("{a:", "}")] {
        let text = format!("{}0{}", opening.repeat(200_000), closing.repeat(200_000));
        let error = parse(&text).unwrap_err().to_string();
        assert!(error.contains("nesting"), "{error}");
    }
}

#[test]
fn rejects_unary_chains_and_invalid_syntax() {
    let unary = format!("{{value: {}1}}", "+".repeat(200_000));
    for text in [unary.as_str(), "{a: 1} garbage", "{a: [1,,2]}", r"{a: '\uZZZZ'}"] {
        let result = parse(text);
        assert!(result.is_err(), "{result:?}");
    }
}

#[test]
fn strings_and_comments_do_not_consume_nesting_budget() {
    let text = format!("{{value: '{}', /* {} */ }}", "[".repeat(1000), "{".repeat(1000));
    assert_eq!(parse(&text).unwrap()["value"], "[".repeat(1000));
}

#[test]
fn non_finite_numbers_are_not_silently_replaced_with_null() {
    for number in ["NaN", "Infinity", "-Infinity", "1e999"] {
        let error = parse(&format!("{{custom: {number}}}")).unwrap_err().to_string();
        assert!(error.contains("finite"), "{error}");
    }
}
