use super::{OwnerEntry, OwnerError};

#[test]
fn owner_entry_deserializes() {
    let json = r#"{"username": "alice", "email": "alice@example.com"}"#;
    let entry: OwnerEntry = serde_json::from_str(json).expect("should deserialize");
    assert_eq!(entry.username, "alice");
    assert_eq!(entry.email, "alice@example.com");
}

#[test]
fn owner_entry_deserializes_array() {
    let json = r#"[
        {"username": "alice", "email": "alice@example.com"},
        {"username": "bob", "email": "bob@example.com"}
    ]"#;
    let entries: Vec<OwnerEntry> = serde_json::from_str(json).expect("should deserialize");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].username, "alice");
    assert_eq!(entries[1].username, "bob");
}

#[test]
fn error_add_args_required_display() {
    let err = OwnerError::AddArgsRequired;
    assert!(err.to_string().contains("Package name and owner are required"));
}

#[test]
fn error_rm_args_required_display() {
    let err = OwnerError::RmArgsRequired;
    assert!(err.to_string().contains("Package name and owner are required"));
}

#[test]
fn error_package_not_found_display() {
    let err = OwnerError::PackageNotFound { package_name: "my-pkg".to_string() };
    assert!(err.to_string().contains("my-pkg"));
    assert!(err.to_string().contains("not found"));
}

#[test]
fn error_unauthorized_display() {
    let err = OwnerError::Unauthorized {
        action: "add owner to".to_string(),
        body: "token expired".to_string(),
    };
    assert!(err.to_string().contains("logged in"));
    assert!(err.to_string().contains("token expired"));
}

#[test]
fn error_forbidden_display() {
    let err = OwnerError::Forbidden {
        action: "remove owner from".to_string(),
        body: "not allowed".to_string(),
    };
    assert!(err.to_string().contains("permission"));
    assert!(err.to_string().contains("not allowed"));
}
