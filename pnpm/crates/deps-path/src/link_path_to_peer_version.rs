#[must_use]
pub fn link_path_to_peer_version(rel_path: &str) -> String {
    let trimmed = rel_path.trim_start_matches('.');

    let mut out = String::with_capacity(rel_path.len());
    let mut last_was_plus = true;
    for ch in trimmed.chars() {
        if !needs_replacing(ch) {
            out.push(ch);
            last_was_plus = false;
            continue;
        }
        if !last_was_plus {
            out.push('+');
            last_was_plus = true;
        }
    }

    let trimmed_end = out.trim_end_matches(['+', '.']).len();
    if trimmed_end > 0 {
        out.truncate(trimmed_end);
        return out;
    }
    if rel_path.is_empty() { String::new() } else { "+".to_string() }
}

/// Convert a `link:` target's path into the filename-safe token pnpm
/// uses as the peer's "version" inside peer-suffix hashes.
///
/// The output must stay stable across pnpm versions so lockfiles
/// don't churn; the encoding replicates what
/// [`filenamify` v4](https://www.npmjs.com/package/filenamify/v/4.3.0)
/// produced for these paths in pnpm <= 10. The encoding is lossy and
/// can collide. Pnpm accepts the rare collision for lockfile
/// stability; see [pnpm/pnpm#11272](https://github.com/pnpm/pnpm/issues/11272).
#[must_use]
/// A character a directory name cannot carry on every platform, which the
/// suffix replaces with a single `+`.
fn needs_replacing(ch: char) -> bool {
    ch.is_control() || matches!(ch, '"' | '*' | '+' | '/' | ':' | '<' | '>' | '?' | '\\' | '|')
}

#[cfg(test)]
mod tests;
