use super::splice_lowered_maps;
use pretty_assertions::assert_eq;
use serde_json::json;

/// A prefix-matching string that doesn't name an unconsumed stash
/// entry is data, not a marker: it stays put, the real markers still
/// splice, and the caller's `remaining` count exposes any theft.
#[test]
fn splice_ignores_decoy_markers() {
    let nonce = "\u{f8ff}pacquet-lowered-map:42:";
    let decoy_high = format!("{nonce}7");
    let decoy_text = format!("{nonce}not-an-index");
    let mut document = json!({
        "a": decoy_high,
        "b": decoy_text,
        "c": format!("{nonce}0"),
    });
    let mut maps = vec![Some(json!({"real": true}))];
    let mut remaining = maps.len();
    splice_lowered_maps(&mut document, nonce, &mut maps, &mut remaining);
    assert_eq!(remaining, 0);
    assert_eq!(document, json!({ "a": decoy_high, "b": decoy_text, "c": {"real": true} }));
}

/// Each stash entry splices at most once; a repeat of a marker is
/// data. And an entry whose marker never surfaces leaves `remaining`
/// non-zero, which sends [`super::to_string`] back to the serial
/// lowering instead of emitting a document with a hole.
#[test]
fn duplicate_markers_and_missing_markers_are_survivable() {
    let nonce = "\u{f8ff}pacquet-lowered-map:42:";
    let mut document = json!({
        "a": format!("{nonce}0"),
        "z": format!("{nonce}0"),
    });
    let mut maps = vec![Some(json!({"real": true})), Some(json!({"orphaned": true}))];
    let mut remaining = maps.len();
    splice_lowered_maps(&mut document, nonce, &mut maps, &mut remaining);
    assert_eq!(remaining, 1, "the orphaned entry's marker never surfaced");
    assert_eq!(document["a"], json!({"real": true}));
    assert_eq!(document["z"], json!(format!("{nonce}0")), "the repeat stays data");
}
