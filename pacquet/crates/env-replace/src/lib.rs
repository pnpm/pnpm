//! Environment-variable substitution for pnpm-style `${VAR}` placeholders.
//!
//! Ports pnpm's [`@pnpm/config.env-replace`](https://github.com/pnpm/components/blob/9c2bd17/config/env-replace/env-replace.ts):
//! occurrences of `${VAR}` (with optional `${VAR:-default}` fallback) are
//! replaced with the value the [`EnvVar`] capability returns for `VAR`.
//! Backslashes immediately preceding the `$` escape the placeholder so
//! it is left as-is.
//!
//! The env lookup is threaded through the [`EnvVar`] capability trait so
//! callers can drive every branch (set, unset, empty) with local fakes
//! instead of mutating the real process environment. Production callers
//! thread [`SystemEnv`] (which delegates to `std::env::var`) through the
//! turbofish slot; `pacquet-config` threads its broader `Host` provider
//! instead, per the DI pattern from
//! [pnpm/pacquet#339](https://github.com/pnpm/pacquet/issues/339).

/// Capability: read a process environment variable as a UTF-8 string.
///
/// `pnpm` resolves `${VAR}` placeholders inside `.npmrc` against the
/// process environment in
/// [`loadNpmrcFiles.ts`](https://github.com/pnpm/pnpm/blob/601317e7a3/config/reader/src/loadNpmrcFiles.ts#L156-L162);
/// the lookup is routed through this trait so unit tests can drive every
/// branch (set, unset, empty) with local fakes instead of mutating the
/// real process environment.
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
/// `pacquet-config` threads its own multi-capability `Host` instead.
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

/// Replace every `${VAR}` (or `${VAR:-default}`) placeholder in `text` with
/// the value [`Sys::var`] returns. Placeholders that have no value and no
/// default become `""` (the literal `${...}` never reaches the caller) and
/// are recorded in the returned `Vec` so the caller can surface each one as
/// a warning.
///
/// Mirrors pnpm's `substituteEnv` fallback in
/// `config/reader/src/loadNpmrcFiles.ts`: leaving an unresolved `${VAR}` in
/// an auth value would later be sent as a literal bearer token, notably
/// under OIDC trusted publishing (<https://github.com/pnpm/pnpm/issues/11513>).
///
/// [`Sys::var`]: EnvVar::var
#[must_use]
pub fn env_replace_lossy<Sys: EnvVar>(text: &str) -> (String, Vec<String>) {
    let bytes = text.as_bytes();
    let mut output = String::with_capacity(text.len());
    let mut unresolved = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let char = bytes[index];
        if char != b'$' {
            output.push(char as char);
            index += 1;
            continue;
        }

        // Count backslashes immediately before this `$` in the *source*.
        // Counting from `output` would conflate trailing `\` in a
        // previously-substituted env value with literal source escapes.
        // Upstream's `(?<!\\)(\\*)\$\{...}` runs on the original input.
        let mut backslashes = 0;
        while backslashes < index && bytes[index - 1 - backslashes] == b'\\' {
            backslashes += 1;
        }

        let Some(end) = find_placeholder_end(bytes, index) else {
            output.push('$');
            index += 1;
            continue;
        };

        // Each pair of backslashes collapses to one literal backslash,
        // matching `(\\*)\$\{...\}` in the JS regex with the escape
        // semantics from `replaceEnvMatch`. The source backslashes are
        // already in `output` from the literal-passthrough loop, so we
        // truncate them off and re-emit half.
        output.truncate(output.len() - backslashes);
        for _ in 0..(backslashes / 2) {
            output.push('\\');
        }

        let placeholder = &text[index..=end];
        if backslashes % 2 == 1 {
            // Odd backslashes: the placeholder is escaped, leave it literal.
            output.push_str(placeholder);
        } else {
            let inside = &text[index + 2..end];
            let (var_name, default) = match inside.find(":-") {
                Some(separator) => (&inside[..separator], Some(&inside[separator + 2..])),
                None => (inside, None),
            };
            let value = Sys::var(var_name).filter(|value| !value.is_empty());
            match (value, default) {
                (Some(value), _) => output.push_str(&value),
                (None, Some(default)) => output.push_str(default),
                (None, None) => unresolved.push(placeholder.to_owned()),
            }
        }
        index = end + 1;
    }
    (output, unresolved)
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
