use super::{
    blob_segment, decode_blob_segment, decode_package_segments, decode_segment, encode_segment,
    namespace_segments, package_segments,
};

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
    assert_eq!(namespace_segments(""), ["default"]);
    assert_eq!(namespace_segments("default"), ["orgs", "default"]);
    assert_eq!(namespace_segments("npm~internal"), ["orgs", "npm!7einternal"]);
}

#[test]
fn every_package_is_two_segments_that_decode_back() {
    for (name, segments) in [
        ("left-pad", ["@", "left-pad"]),
        ("@pnpm-testing/fresh", ["@pnpm-testing", "fresh"]),
        ("@Scope/App", ["@!53cope", "!41pp"]),
        ("library/alpine", ["@", "library!2falpine"]),
        ("@/empty-scope", ["@", "!40!2fempty-scope"]),
        ("@no-slash", ["@", "!40no-slash"]),
    ] {
        assert_eq!(package_segments(name), segments, "{name}");
        assert_eq!(decode_package_segments(segments[0], segments[1]).as_deref(), Some(name));
    }
}

#[test]
fn package_segments_the_layout_cannot_produce_do_not_decode() {
    for (scope, rest) in [("scope", "pkg"), ("@a!2fb", "c"), ("@", "Upper"), ("@!zz", "pkg")] {
        assert_eq!(decode_package_segments(scope, rest), None, "{scope}/{rest}");
    }
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
        assert_eq!(decode_blob_segment(name, segment).as_deref(), Some(file), "{segment}");
    }
}

#[test]
fn blob_segments_the_layout_cannot_produce_do_not_decode() {
    for segment in ["@foo-1.0.0.tgz", "@sha256-aa", ".tgz", "Upper"] {
        assert_eq!(decode_blob_segment("foo", segment), None, "{segment:?}");
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
