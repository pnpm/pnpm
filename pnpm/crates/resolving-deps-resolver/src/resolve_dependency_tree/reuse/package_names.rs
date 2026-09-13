use super::Cow;

/// Normalize an `npm:` alias specifier into the real package name and
/// the wanted range (`("is-positive", "^1.0.0")` for the edge
/// `my-alias@npm:is-positive@^1.0.0`; the range is `"*"` for the
/// spec-less `npm:is-positive` form). A specifier without the `npm:`
/// prefix is returned as-is: the alias is the real package name.
///
/// Mirrors the TypeScript resolver's `unwrapPackageName`; unlike
/// [`fn@real_package_name_of`] it also yields the range and does not
/// special-case the `npm:<range>` form, so the locked-entry check
/// matches its TypeScript counterpart byte for byte.
pub(crate) fn unwrap_package_name<'a>(
    alias: &'a str,
    bare_specifier: &'a str,
) -> (&'a str, &'a str) {
    let Some(rest) = bare_specifier.strip_prefix("npm:") else {
        return (alias, bare_specifier);
    };
    match rest.rfind('@') {
        None | Some(0) => (rest, "*"),
        Some(index) => (&rest[..index], &rest[index + 1..]),
    }
}

/// Resolve the *real* package name an `(alias, bare_specifier)` edge
/// targets — the name update targeting matches against, not the local
/// install alias, which an `npm:` alias or a `jsr:` specifier can
/// differ from. The picker and the lockfile snapshots key on this name.
/// `walk::overlay_lookup_names` builds its candidate set from it.
///
/// `None` when no name can be recovered; the caller reads that as "not
/// a targeted update", since update targets are keyed by package name.
pub fn real_package_name_of<'edge>(
    alias: Option<&'edge str>,
    bare_specifier: Option<&'edge str>,
) -> Option<Cow<'edge, str>> {
    let bare = bare_specifier?;
    if let Some(rest) = bare.strip_prefix("npm:") {
        let alias_keeps_name = alias.is_some_and(|alias| {
            !alias.is_empty() && rest.parse::<node_semver::Range>().is_ok()
        });
        if !alias_keeps_name {
            let last_at = rest
                .bytes()
                .enumerate()
                .rev()
                .find_map(|(i, b)| (b == b'@').then_some(i));
            let name = match last_at {
                Some(idx) if idx >= 1 => &rest[..idx],
                _ => rest,
            };
            return (!name.is_empty()).then_some(Cow::Borrowed(name));
        }
    }
    if bare.starts_with("jsr:") {
        let spec =
            pnpm_resolving_jsr_specifier_parser::parse_jsr_specifier(bare, alias).ok().flatten()?;
        return Some(Cow::Owned(spec.npm_pkg_name));
    }
    alias.map(Cow::Borrowed)
}
