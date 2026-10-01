/// The hint for a `run` that found nothing to execute while a filter option
/// follows the script name, where it reaches the script instead of
/// selecting projects. `None` when no filter option precedes a `--`.
pub(super) fn filter_placement_hint(script_name: &str, args: &[String]) -> Option<String> {
    let filter_option = filter_option(args)?;
    let script = if is_shell_safe(script_name) { script_name } else { "<script>" };
    Some(format!(
        r#"Options after the script name are passed to the script. To select workspace projects, put {filter_option} before it: "pnpm {filter_option} <selector> run {script}"."#,
    ))
}

fn filter_option(args: &[String]) -> Option<&'static str> {
    args.iter()
        .take_while(|arg| *arg != "--")
        .find_map(|arg| {
            if arg.starts_with("-F") || arg == "--filter" || arg.starts_with("--filter=") {
                Some("--filter")
            } else if arg == "--filter-prod" || arg.starts_with("--filter-prod=") {
                Some("--filter-prod")
            } else {
                None
            }
        })
}

/// Whether `text` can be pasted into a POSIX shell or `cmd` as one argument
/// without quoting or expansion.
fn is_shell_safe(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || "_@+=:,./-".contains(ch))
}
