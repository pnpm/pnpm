//! Naming-case predicates and conversions used by `pnpm config`.
//!
//! Provides two case predicates plus the kebab/camel conversions the config
//! command needs, matching `lodash.kebabcase` and the `camelcase` npm package.
//! Only the behavior the config command depends on is reproduced: the inputs
//! are config keys (kebab-case or camelCase ASCII identifiers) and the
//! user-typed keys the command normalizes before validation.

/// Whether `name` is *strictly* kebab-case: at least two `-`-separated
/// segments, each `[a-z][a-z0-9]*`.
#[must_use]
pub fn is_strictly_kebab_case(name: &str) -> bool {
    let mut segments = name.split('-');
    let first = segments.next();
    let second = segments.next();
    if first.is_none() || second.is_none() {
        return false;
    }
    name.split('-').all(is_kebab_segment)
}

fn is_kebab_segment(segment: &str) -> bool {
    let mut chars = segment.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }
    chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit())
}

/// Whether `name` is camelCase: `[a-z][a-zA-Z0-9]*`.
#[must_use]
pub fn is_camel_case(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric())
}

/// Convert `name` to kebab-case, matching `lodash.kebabcase` for the ASCII
/// identifier inputs the config command uses: split into words on case
/// transitions, digit boundaries, and non-alphanumeric separators, lowercase
/// each word, and join with `-`.
#[must_use]
pub fn to_kebab_case(name: &str) -> String {
    words(name).join("-").to_ascii_lowercase()
}

/// Convert `name` to camelCase, matching the `camelcase` npm package for the
/// ASCII identifier inputs the config command uses: split into words the same
/// way as [`to_kebab_case`], lowercase the first word, and capitalize the
/// first letter of each subsequent word.
#[must_use]
pub fn to_camel_case(name: &str) -> String {
    let mut result = String::with_capacity(name.len());
    for (index, word) in words(name).into_iter().enumerate() {
        let lower = word.to_ascii_lowercase();
        if index == 0 {
            result.push_str(&lower);
        } else {
            let mut chars = lower.chars();
            if let Some(first) = chars.next() {
                result.extend(first.to_uppercase());
                result.push_str(chars.as_str());
            }
        }
    }
    result
}

/// Split an ASCII identifier into words, mirroring `lodash`'s `words` for the
/// inputs the config command sees. Word boundaries: any run of
/// non-alphanumeric characters separates words; within an alphanumeric run, a
/// new word starts at a lower→upper transition, an upper→upper-then-lower
/// transition (`XMLHttp` → `XML`, `Http`), and at letter↔digit transitions.
fn words(name: &str) -> Vec<String> {
    let chars: Vec<char> = name.chars().collect();
    let mut result = Vec::new();
    let mut current = String::new();
    let mut prev: Option<char> = None;

    for (index, &char) in chars.iter().enumerate() {
        if !char.is_ascii_alphanumeric() {
            push_word(&mut result, &mut current);
            prev = None;
            continue;
        }
        let starts_word =
            prev.is_some_and(|prev| boundary_before(prev, char, chars.get(index + 1).copied()));
        if starts_word {
            push_word(&mut result, &mut current);
        }
        current.push(char);
        prev = Some(char);
    }

    push_word(&mut result, &mut current);
    result
}

/// Close the word being built, if there is one.
fn push_word(result: &mut Vec<String>, current: &mut String) {
    if !current.is_empty() {
        result.push(std::mem::take(current));
    }
}

/// Whether a word boundary falls *before* `curr` given the previous char
/// `prev` (both alphanumeric, same separator-free run) and the following char
/// `next`.
fn boundary_before(prev: char, curr: char, next: Option<char>) -> bool {
    let prev_lower = prev.is_ascii_lowercase();
    let prev_upper = prev.is_ascii_uppercase();
    let prev_digit = prev.is_ascii_digit();
    let curr_upper = curr.is_ascii_uppercase();
    let curr_digit = curr.is_ascii_digit();

    // letter → digit and digit → letter both start a new word
    if (prev.is_ascii_alphabetic() && curr_digit) || (prev_digit && curr.is_ascii_alphabetic()) {
        return true;
    }
    // lower → upper: camelCase hump (`fetchRetries` → `fetch`, `Retries`)
    if prev_lower && curr_upper {
        return true;
    }
    // upper → upper followed by lower: acronym end (`XMLHttp` → `XML`, `Http`)
    if prev_upper && curr_upper && next.is_some_and(|following| following.is_ascii_lowercase()) {
        return true;
    }
    false
}

#[cfg(test)]
mod tests;
