use super::convert_to_create_name;

#[test]
fn unscoped_unprefixed_gets_create_prefix() {
    assert_eq!(convert_to_create_name("foo"), "create-foo");
}

#[test]
fn unscoped_already_prefixed_is_unchanged() {
    assert_eq!(convert_to_create_name("create-foo"), "create-foo");
}

#[test]
fn unscoped_empty_prefix_is_unchanged() {
    assert_eq!(convert_to_create_name("create-"), "create-");
}

#[test]
fn unscoped_underscore_prefix_gets_double_prefix() {
    assert_eq!(convert_to_create_name("create_no_dash"), "create-create_no_dash");
}

#[test]
fn scoped_unprefixed_gets_create_prefix() {
    assert_eq!(convert_to_create_name("@scope/foo"), "@scope/create-foo");
}

#[test]
fn scoped_already_prefixed_is_unchanged() {
    assert_eq!(convert_to_create_name("@scope/create-foo"), "@scope/create-foo");
}

#[test]
fn scoped_empty_prefix_is_unchanged() {
    assert_eq!(convert_to_create_name("@scope/create-"), "@scope/create-");
}

#[test]
fn scoped_underscore_prefix_gets_double_prefix() {
    assert_eq!(convert_to_create_name("@scope/create_no_dash"), "@scope/create-create_no_dash");
}

#[test]
fn plain_scope_gets_create() {
    assert_eq!(convert_to_create_name("@scope"), "@scope/create");
}

#[test]
fn unscoped_with_version() {
    assert_eq!(convert_to_create_name("foo@2.0.0"), "create-foo@2.0.0");
    assert_eq!(convert_to_create_name("foo@latest"), "create-foo@latest");
}

#[test]
fn scoped_with_version() {
    assert_eq!(convert_to_create_name("@scope/foo@2.0.0"), "@scope/create-foo@2.0.0");
}

#[test]
fn scoped_already_prefixed_with_version() {
    assert_eq!(convert_to_create_name("@scope/create-a@2.0.0"), "@scope/create-a@2.0.0");
}

#[test]
fn plain_scope_with_version() {
    assert_eq!(convert_to_create_name("@scope@2.0.0"), "@scope/create@2.0.0");
    assert_eq!(convert_to_create_name("@scope@next"), "@scope/create@next");
}
