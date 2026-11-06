use super::{
    archive_object_path, blob_segment, decode_segment, encode_segment, namespace_segments,
};
use pnpr_package_name::Ecosystem;

#[test]
fn every_name_becomes_one_segment_that_decodes_back() {
    for (name, encoded) in [
        ("left-pad", "left-pad"),
        ("@scope/pkg", "!40scope!2fpkg"),
        ("library/alpine", "library!2falpine"),
        ("JSONStream", "!4a!53!4f!4e!53tream"),
        ("npm~internal", "npm!7einternal"),
        ("Django-4.0-py3-none-any.whl", "!44jango-4.0-py3-none-any.whl"),
        ("naïve", "na!c3!afve"),
    ] {
        assert_eq!(encode_segment(name), encoded, "{name}");
        assert_eq!(decode_segment(encoded).as_deref(), Some(name), "{encoded}");
    }
}

#[test]
fn names_that_differ_only_in_case_get_different_segments() {
    assert_ne!(encode_segment("Foo"), encode_segment("foo"));
}

#[test]
fn segments_the_encoding_cannot_produce_do_not_decode() {
    for segment in ["Upper", "a/b", "a~b", "!4", "!zz", "!4A", "!ff", "!61", "!2e"] {
        assert_eq!(decode_segment(segment), None, "{segment}");
    }
}

#[test]
fn the_default_namespace_cannot_be_an_org() {
    assert_eq!(namespace_segments("", Ecosystem::Npm), ["default", "npm"]);
    assert_eq!(namespace_segments("", Ecosystem::Cargo), ["default", "cargo"]);
    assert_eq!(namespace_segments("default", Ecosystem::Npm), ["orgs", "default", "npm"]);
    assert_eq!(namespace_segments("acme", Ecosystem::Oci), ["orgs", "acme", "oci"]);
    assert_eq!(
        namespace_segments("npm~internal", Ecosystem::Pypi),
        ["orgs", "npm!7einternal", "pypi"],
    );
}

#[test]
fn a_blob_named_after_its_package_drops_the_name() {
    for (name, file, segment) in [
        ("left-pad", "left-pad-1.0.0.tgz", "1.0.0.tgz"),
        ("@scope/pkg", "pkg-1.0.0-RC.1.sigstore", "1.0.0-!52!43.1.sigstore"),
        ("serde", "serde-1.0.0.crate", "1.0.0.crate"),
        ("library/alpine", "sha256-aa", "sha256-aa"),
        ("my-pkg", "my_pkg-1.0-py3-none-any.whl", "my_pkg-1.0-py3-none-any.whl"),
        ("demo", "demo-1.0.tar.gz", "demo-1.0.tar.gz"),
        ("foo", "foo-.tgz", "@foo-.tgz"),
        ("foo", "foo-..tgz", "@foo-..tgz"),
        ("foo", "foobar-1.0.0.tgz", "@foobar-1.0.0.tgz"),
        ("foo", "1.0.0.tgz", "@1.0.0.tgz"),
    ] {
        assert_eq!(blob_segment(name, file), segment, "{file}");
    }
}

#[test]
fn files_of_ecosystems_without_versioned_names_keep_their_names() {
    for file in [
        "sha256-aa",
        "demo-1.0-py3-none-any.whl",
        "demo-1.0-py3-none-any.whl.metadata",
        "demo-1.0.tar.gz",
        "demo-1.0.zip",
    ] {
        assert_eq!(blob_segment("demo", file), encode_segment(file), "{file}");
    }
}

#[test]
fn an_archive_is_stored_under_a_shard_of_its_digest() {
    assert_eq!(archive_object_path("sha512-ab12.3"), "archives/ab/sha512-ab12.3");
    assert_eq!(archive_object_path("sha256-0f.1"), "archives/0f/sha256-0f.1");
}
