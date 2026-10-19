use super::{
    default_rules::{DEFAULT_PATTERNS, may_match_default_rules},
    rule::IgnoreRule,
};

/// Each case is `(rule, path, partial, minimatch's result)`, recorded from
/// minimatch with the options ignore-walk passes it.
const CASES: &[(&str, &str, bool, bool)] = &[
    ("*", "", false, false),
    ("*", "a/", false, true),
    ("lib/*", "lib/", false, false),
    ("a*", "A", false, true),
    ("lib/!(x)", "lib/", false, true),
    ("[a-c]", "B", false, true),
    ("a{b,c}d", "acd", false, true),
    ("a{b}d", "a{b}d", false, true),
    ("*.js", "x.JS", false, true),
    ("foo/", "foo", false, false),
    ("foo/", "foo", true, true),
    ("!(a)b", "cb", false, false),
    ("*(a|b)c", "ababc", false, true),
    ("+(a)", "", false, false),
    ("?(a)", "", false, true),
    (r"\*", "*", false, true),
    (r"\*", "x", false, false),
    ("[!a]", "b", false, true),
    ("[^a]", "a", false, false),
    ("[]a]", "]", false, true),
    ("[[:digit:]]x", "1x", false, true),
    ("a[", "a[", false, true),
    ("@(a", "@(a", false, true),
    ("l/**", "l/", false, true),
    ("l/?(a)", "l/", false, true),
    ("l/+(a)", "l/", false, false),
    ("l/**/*", "l/", false, false),
    ("l/*/", "l/", false, false),
    ("x/!(a)b", "x/ab", false, false),
    ("x/!(a)b", "x/cb", false, true),
    ("x/!(a)b", "x/b", false, true),
    ("x/!(a|ab)", "x/ab", false, false),
    ("x/a!(b)", "x/a", false, true),
    ("x/a!(b)c", "x/abc", false, false),
    ("x/!(*.js)", "x/.js", false, false),
    ("x/@(a|b)*", "x/bz", false, true),
    ("a/**", "a/.hidden/x", false, true),
    ("x/[A-C]", "x/b", false, true),
    ("x/É", "x/é", false, true),
    ("lib/**/!(*.js.map|.tsbuildinfo)", "lib/utils/foo.js", false, true),
    ("lib/**/!(*.js.map|.tsbuildinfo)", "lib/utils/foo.js.map", false, false),
    ("lib/**/!(*.js.map|.tsbuildinfo)", "lib/.tsbuildinfo", false, false),
    ("/.wafpickle-*", "/.wafpickle-2", false, true),
    ("/.wafpickle-*", "/lib/.wafpickle-2", false, false),
    ("**/._*/**", "/lib/._dir/inner.js", false, true),
    ("**/._*/**", "/lib/dir/inner.js", false, false),
    (".*.swp", "/lib/.a.js.swp", false, true),
    (".*.swp", "/lib/.foo.SWP", false, true),
    (".*.swp", "/lib/a.swp", false, false),
    ("/build/config.gypi", "/build/config.gypi", false, true),
    ("/build/config.gypi", "/sub/build/config.gypi", false, false),
];

#[test]
fn rules_match_like_minimatch() {
    for &(rule, path, partial, expected) in CASES {
        let parsed = IgnoreRule::parse(rule);
        assert_eq!(
            parsed.matches(path, partial),
            expected,
            "rule {rule:?} on {path:?} (partial: {partial})",
        );
    }
}

#[test]
fn leading_exclamation_marks_toggle_negation() {
    assert!(!IgnoreRule::parse("lib").negate);
    assert!(IgnoreRule::parse("!lib").negate);
    assert!(!IgnoreRule::parse("!!lib").negate);
    assert!(IgnoreRule::parse("!(lib)").negate);
}

#[test]
fn many_stars_do_not_backtrack_exponentially() {
    let rule = IgnoreRule::parse(&format!("{}b", "*a".repeat(30)));
    assert!(!rule.matches(&"a".repeat(200), false));
}

#[test]
fn repeated_brace_groups_expand_to_a_bounded_number_of_alternatives() {
    let rule = IgnoreRule::parse(&"{a,b}".repeat(40));
    assert!(rule.matches(&"a".repeat(40), false));
}

#[test]
fn ambiguous_extglob_repetition_is_bounded() {
    let rule = IgnoreRule::parse("+(a|aa)");
    assert!(!rule.matches(&format!("{}b", "a".repeat(200)), false));
}

#[test]
fn deeply_nested_extglobs_parse() {
    let rule = IgnoreRule::parse(&format!("{}a{}", "@(".repeat(100_000), ")".repeat(100_000)));
    assert!(!rule.matches("b", false));
}

#[test]
fn separated_globstars_are_bounded() {
    let rule = IgnoreRule::parse(&format!("{}z", "**/a/".repeat(20)));
    assert!(!rule.matches(&"a/".repeat(60), false));
}

#[test]
fn a_wide_brace_group_expands_to_a_bounded_number_of_alternatives() {
    let rule = IgnoreRule::parse(&format!("x{{{}y}}", ",".repeat(10_000_000)));
    assert!(rule.matches("x", false));
    assert!(!rule.matches("xy", false));
}

#[test]
fn reinclude_path_matches_only_the_path_itself() {
    let rule = IgnoreRule::reinclude_path("dist/[id].js");
    assert!(rule.negate);
    let cases = [
        ("/dist/[id].js", false, true),
        ("/dist", true, true),
        ("/Dist", true, false),
        ("/DIST/[id].js", false, false),
        ("/dist/i.js", false, false),
        ("/dist/[ID].js", false, false),
    ];
    for (path, partial, expected) in cases {
        assert_eq!(rule.matches(path, partial), expected, "{path:?} (partial: {partial})");
    }
}

fn gate_passes(path: &str) -> bool {
    path.split('/').any(may_match_default_rules)
}

#[test]
fn default_rule_gate_is_a_superset_of_the_default_rules() {
    const SAMPLES: &[&str] = &[
        "/.DS_Store",
        "/.npmignore",
        "/lib/.gitignore",
        "/npm-debug.log",
        "/lib/file.orig",
        "/.lock-wscript",
        "/.LOCK-WSCRIPT",
        "/.wafpickle-2",
        "/.WAFPICKLE-2",
        "/build/config.gypi",
        "/BUILD/CONFIG.GYPI",
        "/sub/build/config.gypi",
        "/lib/.a.js.swp",
        "/lib/.foo.SWP",
        "/.index.js.swp",
        "/._a.js",
        "/lib/._A.js",
        "/lib/._dir/inner.js",
        "/LIB/._DIR/INNER.JS",
        "/lib/.DS_Store/child.js",
        "/lib/.ds_store/child.js",
        "/archived-packages/x",
        "/ARCHIVED-PACKAGES/x",
        "/sub/archived-packages/x",
    ];
    for pattern in DEFAULT_PATTERNS {
        let rule = IgnoreRule::parse(pattern);
        let mut matched = false;
        for sample in SAMPLES {
            if rule.matches(sample, false) || rule.matches(sample, true) {
                matched = true;
                assert!(gate_passes(sample), "{pattern:?} matches {sample:?}");
            }
        }
        assert!(matched, "no sample matches {pattern:?}");
    }
}

#[test]
fn default_rule_gate_rejects_ordinary_paths() {
    for path in [
        "index.js",
        "lib/a.js",
        "build/other.js",
        "x.swp",
        "config.gypi.bak",
        "archived-pac\u{212A}ages/data.json",
        ".loc\u{212A}-wscript",
    ] {
        assert!(!gate_passes(path), "{path:?}");
    }
}

#[test]
fn manifest_metadata_lookups_stay_within_the_package() {
    let base = tempfile::tempdir().unwrap();
    let root = base.path().join("pkg");
    let outside = base.path().join("outside");
    std::fs::create_dir_all(root.join("inside")).unwrap();
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("secret"), "secret").unwrap();
    std::fs::write(root.join("inside/file"), "public").unwrap();
    pnpm_fs::symlink_dir(&outside, &root.join("link")).unwrap();
    let mut directories = std::collections::HashMap::new();

    for entry in ["../outside/secret", "/../outside/secret", "link/secret"] {
        assert!(
            super::files_field::entry_metadata(&root, entry, &mut directories).is_none(),
            "{entry}",
        );
    }
    assert!(
        super::files_field::entry_metadata(&root, "/inside/file", &mut directories)
            .unwrap()
            .is_file(),
    );
    assert!(
        super::files_field::entry_metadata(&root, "link", &mut directories).unwrap().is_symlink(),
    );
    assert!(super::files_field::entry_metadata(&root, "", &mut directories).unwrap().is_dir());
}
