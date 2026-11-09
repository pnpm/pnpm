/// The target name and selector of an `npm:` alias. An alias without a
/// selector uses `*`. Other specifiers keep the declared name and selector.
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
