//! Each test runs the real function and checks the postcondition its model
//! states, written as Rust.

use pipe_trait::Pipe;
use proptest::{
    collection::vec,
    prelude::*,
    test_runner::RngSeed,
};
use split_first_char::SplitFirstChar;

fn config() -> ProptestConfig {
    let from_env = ProptestConfig::default();
    let cases = if std::env::var_os("PROPTEST_CASES").is_some() { from_env.cases } else { 512 };
    let rng_seed = match from_env.rng_seed {
        RngSeed::Random => RngSeed::Fixed(2026),
        seed @ RngSeed::Fixed(_) => seed,
    };
    ProptestConfig { cases, rng_seed, failure_persistence: None, ..from_env }
}

/// `utf8::is_boundary(text@, offset)`: some count of `text`'s first
/// characters holds `offset` bytes.
fn is_boundary(text: &str, offset: usize) -> bool {
    text.char_indices()
        .map(|(index, _)| index)
        .chain([text.len()])
        .any(|index| index == offset)
}

/// The characters of `text` whose encoding starts within `bytes`.
fn chars_in(text: &str, bytes: std::ops::Range<usize>) -> String {
    text.char_indices()
        .filter(|(index, _)| bytes.contains(index))
        .map(|(_, character)| character)
        .collect()
}

/// `text@ == Seq::singleton(first).concat(rest@)`, or `text@.len() == 0`.
fn rejoins(text: &str, split: Option<(char, &str)>) -> bool {
    match split {
        None => text.is_empty(),
        Some((first, rest)) => {
            let mut rejoined = String::from(first);
            rejoined.push_str(rest);
            rejoined == text
        }
    }
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn split_first_char_function_matches_model(text in any::<String>()) {
        prop_assert!(rejoins(&text, split_first_char::split_first_char(&text)));
    }

    #[test]
    fn split_first_char_method_matches_model(text in any::<String>()) {
        prop_assert!(rejoins(&text, text.as_str().split_first_char()));
    }

    #[test]
    fn pipe_matches_model(value in any::<u32>()) {
        prop_assert_eq!(value.pipe(u64::from), u64::from(value));
    }

    #[test]
    fn pipe_ref_matches_model(value in any::<u32>()) {
        prop_assert_eq!(value.pipe_ref(|value| u64::from(*value)), u64::from(value));
    }

    #[test]
    fn pipe_mut_matches_model(value in any::<u32>()) {
        let mut piped = value;
        piped.pipe_mut(|piped| *piped = piped.wrapping_add(1));
        prop_assert_eq!(piped, value.wrapping_add(1));
    }

    #[test]
    fn pipe_as_ref_passes_what_as_ref_returns(text in any::<String>()) {
        let passed = text.pipe_as_ref(|param: &str| (param.as_ptr(), param.len()));
        let as_ref: &str = text.as_ref();
        prop_assert_eq!(passed, (as_ref.as_ptr(), as_ref.len()));
    }

    #[test]
    fn pipe_deref_passes_what_deref_returns(text in any::<String>()) {
        let passed = text.pipe_deref(|param: &str| (param.as_ptr(), param.len()));
        let dereferenced: &str = &text;
        prop_assert_eq!(passed, (dereferenced.as_ptr(), dereferenced.len()));
    }

    #[test]
    fn split_once_matches_model(text in "[a@/é]{0,12}", delimiter in prop::sample::select(vec!['@', '/', 'é'])) {
        match text.split_once(delimiter) {
            None => prop_assert!(!text.chars().any(|character| character == delimiter)),
            Some((before, after)) => {
                prop_assert_eq!(format!("{before}{delimiter}{after}"), text.as_str());
                prop_assert!(!before.chars().any(|character| character == delimiter));
            }
        }
    }

    #[test]
    fn split_once_on_a_pair_matches_model(text in r"[a/\\é]{0,12}") {
        let is_separator = |character: char| character == '/' || character == '\\';
        match text.split_once(['/', '\\']) {
            None => prop_assert!(!text.chars().any(is_separator)),
            Some((before, after)) => {
                let separator = text[before.len()..].chars().next().unwrap();
                prop_assert!(is_separator(separator));
                prop_assert_eq!(format!("{before}{separator}{after}"), text.as_str());
                prop_assert!(!before.chars().any(is_separator));
            }
        }
    }

    #[test]
    fn starts_with_and_contains_match_model(text in r"[a/\\é:]{0,8}") {
        let first = text.chars().next();
        prop_assert_eq!(text.starts_with('/'), first == Some('/'));
        prop_assert_eq!(text.starts_with(['/', '\\']), matches!(first, Some('/' | '\\')));
        prop_assert_eq!(text.contains(':'), text.chars().any(|character| character == ':'));
        prop_assert_eq!(
            text.contains([':', '\\']),
            text.chars().any(|character| matches!(character, ':' | '\\')),
        );
    }

    #[test]
    fn string_building_matches_model(pieces in vec("[a/é]{0,4}", 0..6), last in any::<char>()) {
        let mut built = String::new();
        let mut expected = Vec::new();
        for piece in &pieces {
            built.push_str(piece);
            expected.extend(piece.chars());
        }
        built.push(last);
        expected.push(last);
        prop_assert_eq!(built.chars().collect::<Vec<_>>(), expected);
    }

    #[test]
    fn str_range_index_matches_model(text in "[a/é𝄞]{0,6}", start in 0_usize..16, end in 0_usize..16) {
        let in_bounds =
            start <= end && end <= text.len() && is_boundary(&text, start) && is_boundary(&text, end);
        prop_assert_eq!(text.get(start..end).is_some(), in_bounds);
        if in_bounds {
            prop_assert_eq!(&text[start..end], chars_in(&text, start..end));
        }
    }

    #[test]
    fn str_range_from_index_matches_model(text in "[a/é𝄞]{0,6}", start in 0_usize..16) {
        let in_bounds = start <= text.len() && is_boundary(&text, start);
        prop_assert_eq!(text.get(start..).is_some(), in_bounds);
        if in_bounds {
            prop_assert_eq!(&text[start..], chars_in(&text, start..text.len()));
        }
    }

    #[test]
    fn str_range_to_index_matches_model(text in "[a/é𝄞]{0,6}", end in 0_usize..16) {
        let in_bounds = end <= text.len() && is_boundary(&text, end);
        prop_assert_eq!(text.get(..end).is_some(), in_bounds);
        if in_bounds {
            prop_assert_eq!(&text[..end], chars_in(&text, 0..end));
        }
    }

    #[test]
    fn is_empty_matches_model(text in "[a/é]{0,3}") {
        prop_assert_eq!(str::is_empty(&text), text.chars().count() == 0);
        prop_assert_eq!(String::is_empty(&text), text.chars().count() == 0);
    }

    #[test]
    fn slice_contains_matches_model(items in vec(0_u8..8, 0..16), item in 0_u8..8) {
        let found_at_an_index = (0..items.len()).any(|index| items[index] == item);
        prop_assert_eq!(items.contains(&item), found_at_an_index);
    }
}

#[test]
fn split_first_char_models_hold_at_the_edges() {
    for text in ["", "a", "ab", "é", "éa", "𝄞x"] {
        assert!(rejoins(text, split_first_char::split_first_char(text)), "{text:?}");
    }
}

#[test]
fn num_cpus_counts_at_least_one() {
    assert!(num_cpus::get() >= 1);
}

#[test]
fn is_ascii_alphabetic_matches_model_for_every_char() {
    for character in (0..=u32::from(char::MAX)).filter_map(char::from_u32) {
        let code = u32::from(character);
        let letter = (65..=90).contains(&code) || (97..=122).contains(&code);
        assert_eq!(character.is_ascii_alphabetic(), letter, "{character:?}");
    }
}

#[test]
fn non_ascii_characters_encode_to_high_bytes_for_every_char() {
    let mut buffer = [0; 4];
    for character in (0..=u32::from(char::MAX)).filter_map(char::from_u32) {
        let encoded = character.encode_utf8(&mut buffer).as_bytes();
        if !character.is_ascii() {
            assert!(encoded.iter().all(|byte| *byte >= 0x80), "{character:?}");
        }
    }
}
