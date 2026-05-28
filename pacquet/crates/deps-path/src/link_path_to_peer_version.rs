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
mod tests {
    use super::link_path_to_peer_version;

    #[test]
    fn replaces_path_separators_with_plus() {
        assert_eq!(link_path_to_peer_version("packages/b"), "packages+b");
    }

    #[test]
    fn collapses_leading_dot_segments() {
        assert_eq!(link_path_to_peer_version("./packages/b"), "packages+b");
        assert_eq!(link_path_to_peer_version("../packages/b"), "packages+b");
    }

    #[test]
    fn leading_dot_in_filename_is_dropped() {
        assert_eq!(link_path_to_peer_version(".hidden/pkg"), "hidden+pkg");
    }

    #[test]
    fn collapses_runs_of_reserved_chars_into_one_plus() {
        assert_eq!(link_path_to_peer_version("a///b"), "a+b");
    }

    #[test]
    fn windows_separators_collapse() {
        assert_eq!(link_path_to_peer_version(r"a\b\c"), "a+b+c");
    }

    #[test]
    fn external_link_target_under_node_modules_matches_upstream() {
        assert_eq!(
            link_path_to_peer_version("node_modules/@pnpm.e2e/peer-a"),
            "node_modules+@pnpm.e2e+peer-a",
        );
    }

    #[test]
    fn empty_input_returns_empty() {
        assert_eq!(link_path_to_peer_version(""), "");
    }

    #[test]
    fn dot_only_collapses_to_single_plus() {
        assert_eq!(link_path_to_peer_version("."), "+");
        assert_eq!(link_path_to_peer_version(".."), "+");
    }

    #[test]
    fn trailing_dots_and_plusses_are_trimmed() {
        assert_eq!(link_path_to_peer_version("a/b."), "a+b");
        assert_eq!(link_path_to_peer_version("a/b/"), "a+b");
    }

    /// Unicode path segments (multi-byte UTF-8) survive intact. A
    /// byte-wise loop would corrupt them.
    #[test]
    fn non_ascii_path_segments_round_trip() {
        assert_eq!(link_path_to_peer_version("packages/café"), "packages+café");
        assert_eq!(link_path_to_peer_version("パッケージ/foo"), "パッケージ+foo");
        assert_eq!(link_path_to_peer_version("📦/pkg"), "📦+pkg");
    }
}
