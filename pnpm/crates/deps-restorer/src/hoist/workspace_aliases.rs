use pnpm_modules_yaml::HoistKind;
use std::collections::HashSet;

pub(super) fn conflicting_aliases<'a>(
    aliases: impl IntoIterator<Item = (&'a str, HoistKind)>,
) -> HashSet<String> {
    let (mut private, mut public) = aliases
        .into_iter()
        .map(|(alias, kind)| (alias.to_lowercase(), kind))
        .partition::<Vec<_>, _>(|(_, kind)| *kind == HoistKind::Private);
    let mut conflicting = HashSet::new();
    add_conflicting_aliases(&mut private, &mut conflicting);
    add_conflicting_aliases(&mut public, &mut conflicting);
    conflicting
}

fn add_conflicting_aliases(
    aliases: &mut Vec<(String, HoistKind)>,
    conflicting: &mut HashSet<String>,
) {
    aliases.sort_unstable_by(|(left, _), (right, _)| left.split('/').cmp(right.split('/')));
    aliases.dedup_by(|(left, _), (right, _)| left == right);
    let mut active_ancestors: Vec<String> = Vec::new();
    for (alias, _) in aliases.drain(..) {
        retain_ancestors_of(&alias, &mut active_ancestors);
        if !active_ancestors.is_empty() {
            conflicting.extend(active_ancestors.iter().cloned());
            conflicting.insert(alias.clone());
        }
        active_ancestors.push(alias);
    }
}

fn retain_ancestors_of(alias: &str, active_ancestors: &mut Vec<String>) {
    while let Some(ancestor) = active_ancestors.last() {
        if alias
            .strip_prefix(ancestor)
            .is_some_and(|suffix| suffix.starts_with('/'))
        {
            break;
        }
        active_ancestors.pop();
    }
}
