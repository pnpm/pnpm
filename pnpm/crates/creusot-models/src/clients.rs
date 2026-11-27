//! Verified callers of each contract, which fail to prove if a contract is
//! too weak to be used.

use creusot_std::prelude::{
    ensures,
    requires,
};
use pipe_trait::Pipe;
use split_first_char::SplitFirstChar;

#[ensures(match result {
    None => text@.len() == 0,
    Some(first) => text@.len() > 0 && text@[0] == first,
})]
pub fn first_char(text: &str) -> Option<char> {
    match text.split_first_char() {
        Some((first, _)) => Some(first),
        None => None,
    }
}

#[ensures(match result {
    None => text@.len() == 0,
    Some(rest) => rest@.len() + 1 == text@.len(),
})]
pub fn after_first_char(text: &str) -> Option<&str> {
    match split_first_char::split_first_char(text) {
        Some((_, rest)) => Some(rest),
        None => None,
    }
}

#[requires(value@ < u32::MAX@)]
#[ensures(result@ == value@ + 1)]
pub fn piped_successor(value: u32) -> u32 {
    value.pipe(
        #[requires(value@ < u32::MAX@)]
        #[ensures(result@ == value@ + 1)]
        |value: u32| value + 1,
    )
}

#[ensures(result == (items@.len() > 0 && items@[0]@ == 0))]
pub fn starts_with_zero(items: &[usize]) -> bool {
    !items.is_empty() && items[..1].contains(&0)
}
