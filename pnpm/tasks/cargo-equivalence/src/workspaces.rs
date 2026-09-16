/// A Cargo workspace both resolvers are pointed at.
///
/// `files` are written verbatim; any directory whose `Cargo.toml` declares a
/// `[package]` also gets an empty `src/lib.rs`, so a fixture is only its
/// manifests. Requirements are deliberately open rather than pinned: both
/// resolvers read the same index on the same day, so an upstream release
/// changes what they agree on, not whether they agree.
#[derive(Debug, Clone, Copy)]
pub struct Workspace {
    pub name: &'static str,
    pub description: &'static str,
    pub files: &'static [(&'static str, &'static str)],
    pub expectation: Expectation,
}

/// Whether the two resolvers are expected to agree yet.
///
/// A workspace kept here while they do not is a tracked gap, not a red run:
/// it fails only if it starts agreeing, which is the signal that the issue
/// is fixed and the expectation should be raised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expectation {
    Agree,
    Differ { issue: &'static str },
}

pub const WORKSPACES: &[Workspace] = &[
    Workspace {
        name: "napi",
        description: "a napi build whose 3.x line carries prereleases needing a yanked napi-build",
        expectation: Expectation::Agree,
        files: &[(
            "Cargo.toml",
            r#"[package]
name = "napi-stack"
version = "0.1.0"
edition = "2021"

[dependencies]
napi = { version = "3.10.5", default-features = false }
napi-derive = { version = "3.5.10", default-features = false }

[build-dependencies]
napi-build = { version = "2.3.2", default-features = false }
"#,
        )],
    },
    Workspace {
        name: "spanning-range",
        description: "one requirement spanning several compatibility lines",
        expectation: Expectation::Agree,
        files: &[(
            "Cargo.toml",
            r#"[package]
name = "spanning-range"
version = "0.1.0"
edition = "2021"

[dependencies]
rand = { version = ">=0.6, <0.9", default-features = false }
"#,
        )],
    },
    Workspace {
        name: "coexisting-lines",
        description: "a broad and a pinned requirement on one crate, which cargo does not unify",
        expectation: Expectation::Agree,
        files: &[
            (
                "Cargo.toml",
                r#"[workspace]
members = ["wide", "narrow"]
resolver = "2"
"#,
            ),
            (
                "wide/Cargo.toml",
                r#"[package]
name = "wide"
version = "0.1.0"
edition = "2021"

[dependencies]
rand = { version = ">=0.6, <0.8.0", default-features = false }
"#,
            ),
            (
                "narrow/Cargo.toml",
                r#"[package]
name = "narrow"
version = "0.1.0"
edition = "2021"

[dependencies]
rand = { version = "0.6", default-features = false }
"#,
            ),
        ],
    },
    Workspace {
        name: "weak-features",
        description: "syn's `quote?/proc-macro`, which only another member's features turn on",
        expectation: Expectation::Agree,
        files: &[
            (
                "Cargo.toml",
                r#"[workspace]
members = ["gate", "turns-on"]
resolver = "2"
"#,
            ),
            (
                "gate/Cargo.toml",
                r#"[package]
name = "gate"
version = "0.1.0"
edition = "2021"

[dependencies]
syn = { version = "2", default-features = false, features = ["proc-macro", "parsing"] }
"#,
            ),
            (
                "turns-on/Cargo.toml",
                r#"[package]
name = "turns-on"
version = "0.1.0"
edition = "2021"

[dependencies]
syn = { version = "2", default-features = false, features = ["printing", "full"] }
"#,
            ),
        ],
    },
    Workspace {
        name: "feature-activated-deps",
        description: "optional dependencies no selected feature activates",
        expectation: Expectation::Differ { issue: "https://github.com/pnpm/pnpm/issues/14978" },
        files: &[(
            "Cargo.toml",
            r#"[package]
name = "feature-activated-deps"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v4", "serde"] }
"#,
        )],
    },
];

/// The workspaces named by `selected`, or all of them when empty.
pub fn select(selected: &[String]) -> Result<Vec<&'static Workspace>, String> {
    if selected.is_empty() {
        return Ok(WORKSPACES.iter().collect());
    }
    selected
        .iter()
        .map(|name| {
            WORKSPACES
                .iter()
                .find(|workspace| workspace.name == name)
                .ok_or_else(|| name.clone())
        })
        .collect()
}
