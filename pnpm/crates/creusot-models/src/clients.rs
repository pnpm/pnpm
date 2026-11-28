//! Verified callers of each contract, which fail to prove if a contract is
//! too weak to be used.

use crate::str_model::occurs_at;
use creusot_std::prelude::{
    Int,
    Seq,
    ensures,
    proof_assert,
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

#[ensures(match result {
    None => !text@.contains('/'),
    Some(before) => before@.len() < text@.len() && !before@.contains('/'),
})]
pub fn before_first_slash(text: &str) -> Option<&str> {
    match text.split_once('/') {
        Some((before, _)) => Some(before),
        None => None,
    }
}

#[requires(value@ < u32::MAX@)]
#[ensures(result@ == value@ + 1)]
pub fn successor_through_pipe_ref(value: u32) -> u32 {
    value.pipe_ref(
        #[requires((*value)@ < u32::MAX@)]
        #[ensures(result@ == (*value)@ + 1)]
        |value: &u32| *value + 1,
    )
}

#[requires((*value)@ < u32::MAX@)]
#[ensures((^value)@ == (*value)@ + 1)]
pub fn increment_through_pipe_mut(value: &mut u32) {
    value.pipe_mut(
        #[requires((*value)@ < u32::MAX@)]
        #[ensures((^value)@ == (*value)@ + 1)]
        |value: &mut u32| *value += 1,
    );
}

#[ensures(result@ == text@.to_bytes().len())]
pub fn length_through_pipe_as_ref(text: &str) -> usize {
    text.pipe_as_ref(str::len)
}

#[ensures(result@ == text@.to_bytes().len())]
pub fn length_through_pipe_deref(text: &String) -> usize {
    text.pipe_deref(str::len)
}

/// Dividing by the CPU count needs no zero check.
#[ensures(result@ <= total@)]
pub fn share_per_cpu(total: usize) -> usize {
    total / num_cpus::get()
}

#[ensures(forall<index> 0 <= index && index < result@.len() ==>
    result@[index] != '/' && result@[index] != '\\')]
pub fn before_first_separator(text: &str) -> &str {
    match text.split_once(['/', '\\']) {
        Some((before, _)) => before,
        None => text,
    }
}

#[ensures(result == (text@.len() > 0 && (text@[0] == '/' || text@[0] == '\\')))]
pub fn starts_with_separator(text: &str) -> bool {
    text.starts_with(['/', '\\'])
}

#[ensures(result == exists<index> 0 <= index && index < text@.len() && text@[index] == ':')]
pub fn holds_a_colon(text: &str) -> bool {
    text.contains(':')
}

#[ensures(result@ == left@.concat(Seq::singleton('/')).concat(right@))]
pub fn joined_by_slash(left: &str, right: &str) -> String {
    let mut joined = String::new();
    joined.push_str(left);
    joined.push('/');
    joined.push_str(right);
    joined
}

#[ensures(result == ((65 <= letter@ && letter@ <= 90) || (97 <= letter@ && letter@ <= 122)))]
pub fn is_drive_letter(letter: char) -> bool {
    letter.is_ascii_alphabetic()
}

#[ensures(result == exists<at: Int> occurs_at(text@, part@, at))]
pub fn holds_text(text: &str, part: &str) -> bool {
    text.find(part).is_some()
}

#[ensures(match result {
    None => !occurs_at(text@, prefix@, 0)
        || !occurs_at(text@.subsequence(prefix@.len(), text@.len()), suffix@,
            text@.len() - prefix@.len() - suffix@.len()),
    Some(middle) => text@ == prefix@.concat(middle@).concat(suffix@),
})]
pub fn between<'text>(text: &'text str, prefix: &str, suffix: &str) -> Option<&'text str> {
    match text.strip_prefix(prefix) {
        Some(rest) => {
            proof_assert! { rest@ == text@.subsequence(prefix@.len(), text@.len()) };
            rest.strip_suffix(suffix)
        }
        None => None,
    }
}
