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
    fn split_once_matches_model(text in "[a@/é]{0,12}", delimiter in prop::sample::select(vec!['@', '/', 'é'])) {
        match text.split_once(delimiter) {
            None => prop_assert!(!text.contains(delimiter)),
            Some((before, after)) => {
                prop_assert_eq!(format!("{before}{delimiter}{after}"), text.as_str());
                prop_assert!(!before.contains(delimiter));
            }
        }
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
