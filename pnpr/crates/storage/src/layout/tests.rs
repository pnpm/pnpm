use super::{decode_segment, encode_segment, namespace_segments};

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
