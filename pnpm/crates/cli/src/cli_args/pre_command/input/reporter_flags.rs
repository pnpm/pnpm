use super::{
    ArgTable, OsStr, OsString, ReporterFlags, SwitchInput, consumes_next_token, long_value,
};
use crate::cli_args::reporter::{LogLevelSetting, ReporterType};
use clap::ValueEnum;

impl SwitchInput {
    /// The `--reporter` and `--loglevel` flags typed before the first
    /// non-option token, read the way [`Self::from_version_argv`] scans.
    pub(in crate::cli_args::pre_command) fn reporter_flags_from_version_argv(
        argv: &[OsString],
    ) -> ReporterFlags {
        let global_options = ArgTable::top_level(crate::cli_args::grammar());
        let mut flags = ReporterFlags::default();
        let mut index = 1;
        while let Some(token) = argv.get(index).and_then(|token| token.to_str()) {
            if token == "--" || !token.starts_with('-') {
                break;
            }
            let next = argv
                .get(index + 1)
                .map(OsString::as_os_str);
            index += absorb_reporter_flag(&mut flags, token, next)
                .unwrap_or_else(|| if consumes_next_token(token, &global_options) { 2 } else { 1 });
        }
        flags
    }
}

/// Read `--reporter` or `--loglevel`, returning how many argv tokens it
/// consumed, or `None` when the token names neither. A value clap would
/// reject leaves the flag unset.
fn absorb_reporter_flag(
    flags: &mut ReporterFlags,
    token: &str,
    next: Option<&OsStr>,
) -> Option<usize> {
    let parse = |value: &OsStr| value.to_str().map(str::to_owned);
    if let Some((value, width)) = long_value(token, "reporter", next) {
        let parsed = parse(value).and_then(|value| ReporterType::from_str(&value, false).ok());
        flags.reporter = parsed.or(flags.reporter);
        return Some(width);
    }
    let (value, width) = long_value(token, "loglevel", next)?;
    let parsed = parse(value).and_then(|value| LogLevelSetting::from_str(&value, false).ok());
    flags.loglevel = parsed.or(flags.loglevel);
    Some(width)
}
