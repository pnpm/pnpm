use super::{protocol_enabled, read_git_boolean, submodule_protocols};
use std::{collections::HashMap, ffi::OsStr};

#[test]
fn inherited_protocol_allowlists_are_only_narrowed() {
    let policies = HashMap::from([("protocol.file.allow".to_string(), "always".to_string())]);
    for (inherited, expected) in [
        (None, "file:git:http:https:ssh"),
        (Some("https"), "https"),
        (Some("https:ext"), "https"),
        (Some("ssh:file"), "file:ssh"),
        (Some(""), ""),
        (Some("ext"), ""),
    ] {
        assert_eq!(submodule_protocols(inherited.map(OsStr::new), &policies), expected);
    }
}

#[test]
fn recursive_user_protocol_policies_remain_restricted() {
    let policies = HashMap::new();
    assert_eq!(submodule_protocols(None, &policies), "git:http:https:ssh");
    for policy in ["never", "user", "NEVER", "User"] {
        let policies = HashMap::from([("protocol.allow".to_string(), policy.to_string())]);
        assert_eq!(submodule_protocols(None, &policies), "");
    }
}

#[test]
fn protocol_policy_values_are_case_insensitive() {
    for policy in ["always", "ALWAYS", "Always"] {
        let policies = HashMap::from([("protocol.allow".to_string(), policy.to_string())]);
        assert_eq!(submodule_protocols(None, &policies), "file:git:http:https:ssh");
        let policies = HashMap::from([
            ("protocol.allow".to_string(), "never".to_string()),
            ("protocol.https.allow".to_string(), policy.to_string()),
        ]);
        assert_eq!(submodule_protocols(None, &policies), "https");
    }
}

#[test]
fn top_level_fetches_preserve_user_protocol_policies() {
    for (policy, from_user, expected) in [
        ("always", false, true),
        ("user", true, true),
        ("user", false, false),
        ("never", true, false),
    ] {
        let policies = HashMap::from([("protocol.file.allow".to_string(), policy.to_string())]);
        assert_eq!(protocol_enabled("file", &policies, from_user), expected);
    }
}

#[test]
fn caller_protocol_flags_use_git_boolean_parsing() {
    let root = tempfile::tempdir().unwrap();
    for (value, expected) in [
        ("0", false),
        ("false", false),
        ("no", false),
        ("off", false),
        ("", false),
        ("1", true),
        ("true", true),
        ("yes", true),
        ("on", true),
        ("2", true),
    ] {
        assert_eq!(
            read_git_boolean(std::path::Path::new("git"), OsStr::new(value), root.path()).unwrap(),
            expected,
        );
    }
    assert!(
        read_git_boolean(std::path::Path::new("git"), OsStr::new("invalid"), root.path()).is_err(),
    );
}

#[cfg(unix)]
#[test]
fn configured_git_binary_supplies_the_clone_policy() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let binary = root.path().join("custom-git");
    std::fs::write(
        &binary,
        r"#!/bin/sh
printf 'protocol.file.allow\nnever\000'
",
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let protocols = super::read_allowed_git_protocols_with(&binary, root.path()).unwrap();
    assert!(
        !protocols
            .split(':')
            .any(|protocol| protocol == "file"),
        "{protocols}",
    );
}
