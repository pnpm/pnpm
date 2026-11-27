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

#[test]
fn num_cpus_counts_at_least_one() {
    assert!(num_cpus::get() >= 1);
}
