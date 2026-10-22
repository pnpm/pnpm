use super::rule::IgnoreRule;
use std::sync::LazyLock;

/// The rules npm-packlist's [`defaults`](https://github.com/npm/npm-packlist/blob/v10.0.4/lib/index.js)
/// apply in every directory, before the directory's own ignore file. They are
/// soft: a later negation, a `files` entry, or a `main` / `bin` path
/// re-includes a file they exclude. Like minimatch's `nocase` regex for
/// these patterns, they ignore ASCII case only.
static DEFAULT_RULES: LazyLock<Vec<IgnoreRule>> = LazyLock::new(|| {
    DEFAULT_PATTERNS
        .into_iter()
        .map(IgnoreRule::parse_ascii_case)
        .collect()
});

pub(super) const DEFAULT_PATTERNS: [&str; 13] = [
    ".npmignore",
    ".gitignore",
    "npm-debug.log",
    ".DS_Store",
    "*.orig",
    "/.lock-wscript",
    "/.wafpickle-*",
    "/build/config.gypi",
    ".*.swp",
    "._*",
    "**/._*/**",
    "**/.DS_Store/**",
    "/archived-packages/**",
];

/// Whether a path segment equals a default rule's name, or matches its prefix
/// or suffix pattern, compared ASCII-case-insensitively as npm compares them.
/// The default rules are evaluated only for paths with such a segment.
pub(super) fn may_match_default_rules(segment: &str) -> bool {
    let bytes = segment.as_bytes();
    let starts_with = |prefix: &str| {
        bytes
            .get(..prefix.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(prefix.as_bytes()))
    };
    let ends_with = |suffix: &str| {
        bytes
            .len()
            .checked_sub(suffix.len())
            .and_then(|start| bytes.get(start..))
            .is_some_and(|tail| tail.eq_ignore_ascii_case(suffix.as_bytes()))
    };
    starts_with("._")
        || (starts_with(".") && ends_with(".swp"))
        || starts_with(".wafpickle-")
        || ends_with(".orig")
        || [
            "config.gypi",
            ".lock-wscript",
            "archived-packages",
            ".ds_store",
            ".npmignore",
            ".gitignore",
            "npm-debug.log",
        ]
        .iter()
        .any(|name| segment.eq_ignore_ascii_case(name))
}

/// A directory's default rules, see [`DEFAULT_RULES`].
#[derive(Clone, Copy)]
pub(super) struct DirDefaults {
    rules: &'static [IgnoreRule],
    /// Whether a segment of the directory's package-relative path passes
    /// [`may_match_default_rules`].
    pub(super) has_candidate_segment: bool,
}

impl DirDefaults {
    pub(super) fn root() -> Self {
        Self { rules: &DEFAULT_RULES, has_candidate_segment: false }
    }

    pub(super) fn child(self, name: &str) -> Self {
        Self { rules: &DEFAULT_RULES, has_candidate_segment: self.entry_may_match(name) }
    }

    /// Whether the path of the entry `name` of this directory has a segment
    /// the default rules may match.
    pub(super) fn entry_may_match(self, name: &str) -> bool {
        self.has_candidate_segment || may_match_default_rules(name)
    }

    pub(super) fn rules(self, may_match: bool) -> &'static [IgnoreRule] {
        if may_match { self.rules } else { &[] }
    }
}
