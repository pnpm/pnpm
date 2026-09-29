//! Environment-variable substitution for pnpm-style `${VAR}` placeholders.
//!
//! Occurrences of `${VAR}` (with optional `${VAR-default}` or `${VAR:-default}` fallback,
//! or npm's `${VAR?}`, which falls back to `""`) are replaced with the value the
//! [`EnvVar`] capability returns for `VAR`.
//! Backslashes immediately preceding the `$` escape the placeholder so
//! it is left as-is.
//!
//! The env lookup is threaded through the [`EnvVar`] capability trait so
//! callers can drive every branch (set, unset, empty) with local fakes
//! instead of mutating the real process environment. Production callers
//! thread [`SystemEnv`] (which delegates to `std::env::var`) through the
//! turbofish slot; `pnpm-config` threads its broader `Host` provider
//! instead, per the DI pattern from
//! [pnpm/pacquet#339](https://github.com/pnpm/pacquet/issues/339).

/// Capability: read a process environment variable as a UTF-8 string.
///
/// `${VAR}` placeholders inside `.npmrc` are resolved against the
/// process environment; the lookup is routed through this trait so unit
/// tests can drive every branch (set, unset, empty) with local fakes
/// instead of mutating the real process environment.
pub trait EnvVar {
    /// Return the value of the named environment variable, or `None`
    /// when it is unset. Implementations should treat invalid UTF-8
    /// as `None` to match `std::env::var`'s behaviour, which is what
    /// pnpm itself observes via Node's `process.env`.
    fn var(name: &str) -> Option<String>;

    /// Enumerate every `(name, value)` environment variable pair.
    ///
    /// Used by consumers that must match env vars by prefix rather than
    /// by exact name (e.g. URL-scoped `npm_config_//…` auth settings,
    /// where the host is part of the variable name). Defaults to an
    /// empty set so existing fakes that only implement [`EnvVar::var`]
    /// keep compiling; production providers override it.
    #[must_use]
    fn vars() -> Vec<(String, String)> {
        Vec::new()
    }
}

/// Production [`EnvVar`] provider: reads the real process environment via
/// [`std::env::var`].
///
/// Consumers that don't have their own capability provider thread this
/// through the turbofish slot (e.g. `env_replace_lossy::<SystemEnv>(raw)`).
/// `pnpm-config` threads its own multi-capability `Host` instead.
pub struct SystemEnv;

impl EnvVar for SystemEnv {
    fn var(name: &str) -> Option<String> {
        std::env::var(name).ok()
    }

    fn vars() -> Vec<(String, String)> {
        // `std::env::vars()` panics if any name/value is not valid UTF-8.
        // Iterate the OsString form and drop non-UTF-8 entries instead,
        // matching `var`'s `std::env::var(..).ok()` (which yields `None`
        // for non-UTF-8).
        std::env::vars_os()
            .filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().ok()?)))
            .collect()
    }
}

/// Replace `${VAR}`, `${VAR-default}`, and `${VAR:-default}` placeholders with
/// the value [`Sys::var`] returns. Placeholders that have no value and no
/// default become `""` (the literal `${...}` never reaches the caller) and
/// are recorded in the returned `Vec` so the caller can surface each one as
/// a warning. npm's optional `${VAR?}` form defaults to `""`, so it is never
/// recorded.
///
/// Recording each unresolved placeholder matters because leaving an
/// unresolved `${VAR}` in an auth value would later be sent as a literal
/// bearer token, notably under OIDC trusted publishing
/// (<https://github.com/pnpm/pnpm/issues/11513>).
///
/// [`Sys::var`]: EnvVar::var
#[must_use]
pub fn env_replace_lossy<Sys: EnvVar>(text: &str) -> (String, Vec<String>) {
    let bytes = text.as_bytes();
    let mut output = String::with_capacity(text.len());
    let mut unresolved = Vec::new();
    let mut index = 0;
    let mut literal_start = 0;
    while index < bytes.len() {
        let Some(placeholder) = placeholder_at(bytes, index) else {
            index += 1;
            continue;
        };
        // The escape backslashes are held back from the literal span and
        // re-emitted halved: each pair collapses to one literal backslash.
        output.push_str(&text[literal_start..index - placeholder.backslashes]);
        for _ in 0..(placeholder.backslashes / 2) {
            output.push('\\');
        }
        let raw = &text[index..=placeholder.end];
        if placeholder.backslashes % 2 == 1 {
            // Odd backslashes: the placeholder is escaped, leave it literal.
            output.push_str(raw);
        } else {
            expand_placeholder::<Sys>(raw, &mut output, &mut unresolved);
        }
        index = placeholder.end + 1;
        literal_start = index;
    }
    output.push_str(&text[literal_start..]);
    (output, unresolved)
}

/// Error returned when [`env_replace`] encounters a placeholder that has no
/// value and no default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedEnvVar {
    pub placeholder: String,
}

impl std::fmt::Display for UnresolvedEnvVar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Failed to replace env in config: {}", self.placeholder)
    }
}

impl std::error::Error for UnresolvedEnvVar {}

/// Replace every `${VAR}` (or `${VAR-default}` / `${VAR:-default}`) placeholder
/// in `text` with the env value resolved from [`Sys::var`].
/// Returns an error on the first placeholder that has no value and no default.
///
/// [`Sys::var`]: EnvVar::var
pub fn env_replace<Sys: EnvVar>(text: &str) -> Result<String, UnresolvedEnvVar> {
    let (substituted, unresolved) = env_replace_lossy::<Sys>(text);
    if let Some(placeholder) = unresolved.into_iter().next() {
        return Err(UnresolvedEnvVar { placeholder });
    }
    Ok(substituted)
}

/// The `${...}` placeholders of `text`, as byte ranges, leaving out the ones
/// a backslash escapes.
///
/// A caller that resolves placeholders itself, rather than taking the whole
/// substituted string, reads them from here so it agrees with
/// [`env_replace_lossy`] about what a placeholder is — an unfinished `${`
/// among them, which is text rather than the opening of one.
#[must_use]
pub fn placeholder_ranges(text: &str) -> Vec<std::ops::Range<usize>> {
    let bytes = text.as_bytes();
    let mut ranges = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let Some(placeholder) = placeholder_at(bytes, index) else {
            index += 1;
            continue;
        };
        if placeholder.backslashes % 2 == 0 {
            ranges.push(index..placeholder.end + 1);
        }
        index = placeholder.end + 1;
    }
    ranges
}

/// A `${...}` placeholder, and the backslashes written before it.
struct Placeholder {
    /// Index of the closing `}`.
    end: usize,
    /// Backslashes immediately before the `$`.
    backslashes: usize,
}

/// The placeholder starting at `index`, if one starts there.
///
/// The escape count comes from the *source*: counting it from the output
/// would conflate a trailing `\` in a previously-substituted env value with a
/// literal source escape.
fn placeholder_at(bytes: &[u8], index: usize) -> Option<Placeholder> {
    if bytes[index] != b'$' {
        return None;
    }
    let mut backslashes = 0;
    while backslashes < index && bytes[index - 1 - backslashes] == b'\\' {
        backslashes += 1;
    }
    let end = find_placeholder_end(bytes, index)?;
    Some(Placeholder { end, backslashes })
}

/// Substitute one environment placeholder, recording a name that
/// neither the environment nor a default resolves.
fn expand_placeholder<Sys: EnvVar>(
    placeholder: &str,
    output: &mut String,
    unresolved: &mut Vec<String>,
) {
    let inside = &placeholder[2..placeholder.len() - 1];
    let (var_name, default, default_on_empty) = match inside.split_once('-') {
        Some((name, default)) => match name.strip_suffix(':') {
            Some(name) => (name, Some(default), true),
            None => (name, Some(default), false),
        },
        None => match optional_var_name(inside) {
            Some(var_name) => (var_name, Some(""), false),
            None => (inside, None, false),
        },
    };
    let value = Sys::var(var_name)
        .filter(|value| !value.is_empty() || (default.is_some() && !default_on_empty));
    match (value, default) {
        (Some(value), _) => output.push_str(&value),
        (None, Some(default)) => output.push_str(default),
        (None, None) => unresolved.push(placeholder.to_owned()),
    }
}

/// The `NAME` of an npm-style optional `${NAME?}` placeholder. Suffixes
/// ending in `-` are excluded so `${NAME-?}` is handled as a dash default.
fn optional_var_name(inside: &str) -> Option<&str> {
    inside
        .strip_suffix('?')
        .filter(|name| !name.is_empty() && !name.contains('?') && !name.ends_with('-'))
}

/// Return the index of the closing `}` for a `${...}` starting at `start`.
/// Returns `None` if `text[start..]` is not a well-formed placeholder
/// (no opening `{` immediately after `$`, an empty body, or a stray `$`
/// or `{` inside the body).
fn find_placeholder_end(bytes: &[u8], start: usize) -> Option<usize> {
    if bytes.get(start + 1)? != &b'{' {
        return None;
    }
    let body_start = start + 2;
    let mut cursor = body_start;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'}' if cursor > body_start => return Some(cursor),
            b'$' | b'{' | b'}' => return None,
            _ => cursor += 1,
        }
    }
    None
}

#[cfg(test)]
mod tests;
