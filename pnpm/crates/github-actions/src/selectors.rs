use pnpm_matcher::{Matcher, create_matcher};

#[must_use]
pub fn is_selector(selector: &str) -> bool {
    let pattern = selector.strip_prefix('!').unwrap_or(selector);
    !pattern.starts_with('@') && pattern.contains('/')
}

#[must_use]
pub fn normalize_selector(selector: &str) -> String {
    if !is_selector(selector) {
        return selector.to_string();
    }
    selector
        .rsplit_once('@')
        .map_or(selector, |(name, _)| name)
        .to_string()
}

#[must_use]
pub fn selector_matcher(selectors: &[String]) -> Option<Matcher> {
    if selectors.is_empty() {
        return None;
    }
    Some(create_matcher(
        &selectors
            .iter()
            .map(|selector| normalize_selector(selector))
            .collect::<Vec<_>>(),
    ))
}
