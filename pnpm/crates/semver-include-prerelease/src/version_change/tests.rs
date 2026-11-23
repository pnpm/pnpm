use super::{
    VersionChange,
    classify_version_change,
    split_at_changed_field,
};
use node_semver::Version;

fn v(text: &str) -> Version {
    Version::parse(text).unwrap()
}

#[test]
fn classifies_the_highest_field_that_changed() {
    assert_eq!(classify_version_change(&v("1.0.0"), &v("2.0.0")), VersionChange::Breaking);
    assert_eq!(classify_version_change(&v("1.0.0"), &v("1.1.0")), VersionChange::Feature);
    assert_eq!(classify_version_change(&v("1.0.0"), &v("1.0.1")), VersionChange::Fix);
    assert_eq!(classify_version_change(&v("1.0.0"), &v("1.0.0")), VersionChange::None);
    assert_eq!(classify_version_change(&v("1.0.0-alpha.1"), &v("1.0.0")), VersionChange::Unknown);
}

#[test]
fn splits_before_the_changed_field() {
    assert_eq!(split_at_changed_field("2.0.0", VersionChange::Breaking), Some(("", "2.0.0")));
    assert_eq!(split_at_changed_field("4.18.1", VersionChange::Feature), Some(("4.", "18.1")));
    assert_eq!(split_at_changed_field("2.1.3", VersionChange::Fix), Some(("2.1.", "3")));
    assert_eq!(split_at_changed_field("1.0.0", VersionChange::Unknown), None);
}
