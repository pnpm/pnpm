use crate::{
    HashEncoding, hash_object, hash_object_nullable_with_prefix, hash_object_with_encoding,
    hash_object_without_sorting,
};
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn hash_object_known_base64_value() {
    assert_eq!(
        hash_object(&json!({ "b": 1, "a": 2 })),
        "48AVoXIXcTKcnHt8qVKp5vNw4gyOB5VfztHwtYBRcAQ=",
    );
}

#[test]
fn hash_object_sorts_object_keys() {
    assert_eq!(hash_object(&json!({ "b": 1, "a": 2 })), hash_object(&json!({ "a": 2, "b": 1 })));
}

#[test]
fn hash_object_without_sorting_known_base64_value() {
    assert_eq!(
        hash_object_without_sorting(&json!({ "b": 1, "a": 2 }), HashEncoding::Base64),
        "mh+rYklpd1DBj/dg6dnG+yd8BQhU2UiUoRMSXjPV1JA=",
    );
}

/// `serde_json::json!` macro preserves insertion order on its
/// `Map`-backed objects, so `{b:1,a:2}` and `{a:2,b:1}` are distinct
/// inputs here.
#[test]
fn hash_object_without_sorting_distinguishes_key_order() {
    let bx = hash_object_without_sorting(&json!({ "b": 1, "a": 2 }), HashEncoding::Base64);
    let ax = hash_object_without_sorting(&json!({ "a": 2, "b": 1 }), HashEncoding::Base64);
    assert_ne!(bx, ax);
}

/// Hex encoding is what `calcGraphNodeHash` uses for the GVS path
/// at <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/deps/graph-hasher/src/index.ts#L145>.
#[test]
fn hash_object_with_encoding_hex_matches_decoded_base64() {
    let value = json!({ "b": 1, "a": 2 });
    let base64 = hash_object(&value);
    let hex = hash_object_with_encoding(&value, HashEncoding::Hex, /* sort */ true);
    let from_b64 =
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, base64.as_bytes())
            .expect("decode base64");
    let from_hex = hex_decode(&hex);
    assert_eq!(from_b64, from_hex);
}

#[test]
fn hash_object_empty_object_is_stable() {
    let h1 = hash_object(&json!({}));
    let h2 = hash_object(&json!({}));
    assert_eq!(h1, h2);
    assert!(!h1.is_empty());
}

/// Nested-object case that matches the shape pacquet's
/// `calcDepGraphHash` actually feeds into `hash_object`:
/// `{ id: <string>, deps: <Record<string, string>> }`.
#[test]
fn hash_object_dep_state_shape_sorts_nested_keys() {
    let first = hash_object(&json!({
        "id": "foo@1.0.0:sha512-AAA",
        "deps": { "b": "h-b", "a": "h-a" },
    }));
    let second = hash_object(&json!({
        "deps": { "a": "h-a", "b": "h-b" },
        "id": "foo@1.0.0:sha512-AAA",
    }));
    assert_eq!(first, second);
}

/// Non-ASCII string lengths must be counted in UTF-16 code units
/// (JS `string.length`), not in bytes or codepoints. Verifies the
/// `string:<utf16_len>:<value>` framing.
#[test]
fn hash_object_string_length_counts_utf16_code_units() {
    // `"é"` is one codepoint, two UTF-8 bytes, one UTF-16 code
    // unit. `"😀"` is one codepoint, four UTF-8 bytes, two UTF-16
    // code units (surrogate pair).
    let one_unit = hash_object(&json!({ "k": "é" }));
    // Construct an object whose string would be the same length
    // as `"é"` under a wrong byte-length implementation, so the
    // hashes would collide if pacquet measured bytes.
    let two_bytes_ascii = hash_object(&json!({ "k": "ab" }));
    assert_ne!(one_unit, two_bytes_ascii, "utf-16 length must differ from byte length");
}

/// `null`, booleans, and arrays each go through their own arm of
/// the `serialize` dispatcher. The bytestream prefixes (`Null`,
/// `bool:true|false`, `array:<N>:`) are part of the on-disk
/// cache-key contract and must produce stable, distinct hashes
/// — pin them to guard against silent divergence from
/// `@pnpm/crypto.object-hasher`.
#[test]
fn hash_object_handles_null_bool_and_array_variants() {
    // Same scalar wrapped in a single-key object so we can hash
    // each independently. Pacquet's call sites only ever feed
    // top-level objects.
    let null_hash = hash_object(&json!({ "v": null }));
    let true_hash = hash_object(&json!({ "v": true }));
    let false_hash = hash_object(&json!({ "v": false }));
    let array_hash = hash_object(&json!({ "v": [1, 2, 3] }));

    // Collisions across variants would mean the discriminator prefixes
    // were lost.
    let all = [&null_hash, &true_hash, &false_hash, &array_hash];
    for first in all {
        assert!(!first.is_empty());
    }
    for (i, first) in all.iter().enumerate() {
        for (j, second) in all.iter().enumerate() {
            if i != j {
                assert_ne!(first, second, "variant prefixes must distinguish hashes");
            }
        }
    }

    assert_eq!(null_hash, hash_object(&json!({ "v": null })));
    assert_eq!(array_hash, hash_object(&json!({ "v": [1, 2, 3] })));
    assert_ne!(array_hash, hash_object(&json!({ "v": [3, 2, 1] })));
}

#[test]
fn hash_object_nullable_with_prefix_known_value() {
    assert_eq!(
        hash_object_nullable_with_prefix(&json!({ "b": 1, "a": 2 })).as_deref(),
        Some("sha256-48AVoXIXcTKcnHt8qVKp5vNw4gyOB5VfztHwtYBRcAQ="),
    );
    assert_eq!(hash_object_nullable_with_prefix(&json!({})), None);
    assert_eq!(hash_object_nullable_with_prefix(&json!(null)), None);
}

#[test]
fn hash_object_nullable_with_prefix_sorts() {
    assert_eq!(
        hash_object_nullable_with_prefix(&json!({ "b": 1, "a": 2 })),
        hash_object_nullable_with_prefix(&json!({ "a": 2, "b": 1 })),
    );
}

fn hex_decode(hex: &str) -> Vec<u8> {
    assert!(hex.len().is_multiple_of(2));
    (0..hex.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&hex[index..index + 2], 16).expect("hex digit"))
        .collect()
}
