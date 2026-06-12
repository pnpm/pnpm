/// Convert a `link:` target's path into the filename-safe token pnpm
/// uses as the peer's "version" inside peer-suffix hashes.
///
/// Mirrors upstream's
/// [`linkPathToPeerVersion`](https://github.com/pnpm/pnpm/blob/094aa6e57b/installing/deps-resolver/src/linkPathToPeerVersion.ts).
/// The output must stay stable across pnpm versions so lockfiles
/// don't churn; the encoding replicates what
/// [`filenamify` v4](https://www.npmjs.com/package/filenamify/v/4.3.0)
/// produced for these paths in pnpm <= 10. The encoding is lossy and
/// can collide — `packages/b`, `./packages/b`, and `../packages/b`
/// all collapse to `packages+b`, and `.hidden/pkg` collapses to
/// `hidden+pkg`. Pnpm accepts the rare collision for lockfile
/// stability; see [pnpm/pnpm#11272](https://github.com/pnpm/pnpm/issues/11272).
#[must_use]
pub fn link_path_to_peer_version(rel_path: &str) -> String {
    let trimmed = rel_path.trim_start_matches('.');

    let mut out = String::with_capacity(rel_path.len());
    let mut last_was_plus = true;
    for ch in trimmed.chars() {
        let replace = ch.is_control()
            || matches!(ch, '"' | '*' | '+' | '/' | ':' | '<' | '>' | '?' | '\\' | '|');
        if replace {
            if !last_was_plus {
                out.push('+');
                last_was_plus = true;
            }
        } else {
            out.push(ch);
            last_was_plus = false;
        }
    }

    let trimmed_end = out.trim_end_matches(['+', '.']).len();
    if trimmed_end > 0 {
        out.truncate(trimmed_end);
        return out;
    }
    if rel_path.is_empty() { String::new() } else { "+".to_string() }
}

#[cfg(test)]
mod tests;
