use pnpm_matcher::{Matcher, create_matcher};

#[must_use]
pub fn is_selector(selector: &str) -> bool {
    let pattern = selector.strip_prefix('!').unwrap_or(selector);
    let Some((owner, rest)) = pattern.split_once('/') else {
        return false;
    };
    !owner.is_empty() && !rest.is_empty() && !owner.starts_with('@') && !owner.contains(':')
}

#[must_use]
pub fn normalize_selector(selector: &str) -> String {
    if !is_selector(selector) {
        return selector.to_string();
    }
    match selector.rsplit_once('@') {
        Some((name, _)) => name.to_string(),
        None => selector.to_string(),
    }
}

fn expand_selector(selector: &str) -> Vec<String> {
    let normalized = normalize_selector(selector);
    if !is_selector(&normalized) {
        return vec![normalized];
    }
    let bare = normalized.strip_prefix('!').unwrap_or(&normalized);
    if !bare.ends_with('*')
        && !bare
            .split_once('/')
            .is_some_and(|(_, repo)| repo.contains('/'))
    {
        let subpath = format!("{normalized}/*");
        return vec![normalized, subpath];
    }
    vec![normalized]
}

#[must_use]
pub fn selector_matcher(selectors: &[String]) -> Option<Matcher> {
    if selectors.is_empty() {
        return None;
    }
    let patterns = selectors
        .iter()
        .flat_map(|selector| expand_selector(selector))
        .collect::<Vec<_>>();
    Some(create_matcher(&patterns))
}
