//! Expected strings are the "build" vectors of `package-url/purl-spec`
//! (`tests/types/{npm,cargo,pypi,oci}-test.json`), where they apply.

use super::{Purl, PurlError};
use pretty_assertions::assert_eq;

#[test]
fn builds_npm_purls() {
    assert_eq!(
        Purl::npm("@angular/animation", "12.3.1").unwrap().as_str(),
        "pkg:npm/%40angular/animation@12.3.1",
    );
    assert_eq!(Purl::npm("foobar", "12.3.1").unwrap().as_str(), "pkg:npm/foobar@12.3.1");
    assert_eq!(
        Purl::npm("@example/widgets", "1.2.0-beta.1+build.5").unwrap().as_str(),
        "pkg:npm/%40example/widgets@1.2.0-beta.1%2Bbuild.5",
    );
    for name in ["@scope", "@/name", "@scope/"] {
        assert_eq!(
            Purl::npm(name, "1.0.0"),
            Err(PurlError::InvalidNpmName { name: name.to_owned() }),
        );
    }
}

#[test]
fn builds_npm_variant_purls() {
    assert_eq!(
        Purl::npm_variant("example-native-addon", "2.3.0", "linux-x64-musl").unwrap().as_str(),
        "pkg:npm/example-native-addon@2.3.0?variant=linux-x64-musl",
    );
    assert_eq!(
        Purl::npm_variant("@example/cli", "4.0.0", "linux-x64+linux-x64-musl").unwrap().as_str(),
        "pkg:npm/%40example/cli@4.0.0?variant=linux-x64%2Blinux-x64-musl",
    );
}

#[test]
fn builds_cargo_purls() {
    assert_eq!(Purl::cargo("rand", "0.7.2").unwrap().as_str(), "pkg:cargo/rand@0.7.2");
    assert_eq!(Purl::cargo("serde", "1.0.210").unwrap().as_str(), "pkg:cargo/serde@1.0.210");
}

#[test]
fn builds_pypi_purls_for_one_distribution_file() {
    assert_eq!(
        Purl::pypi_file("django", "1.11.1", "Django-1.11.1-py2.py3-none-any.whl").unwrap().as_str(),
        "pkg:pypi/django@1.11.1?file_name=Django-1.11.1-py2.py3-none-any.whl",
    );
    assert_eq!(
        Purl::pypi_file("Django_Package", "1.11.1.dev1", "Django_Package-1.11.1.dev1.tar.gz")
            .unwrap()
            .as_str(),
        "pkg:pypi/django-package@1.11.1.dev1?file_name=Django_Package-1.11.1.dev1.tar.gz",
    );
}

#[test]
fn builds_oci_purls_for_a_tag_binding() {
    assert_eq!(
        Purl::oci_tag("acme/app", "sha256:244fd47e07d10", "1.0").unwrap().as_str(),
        "pkg:oci/app@sha256:244fd47e07d10?tag=1.0",
    );
    assert_eq!(
        Purl::oci_tag("distroless/Static", "SHA256:244FD47E07D10", "latest").unwrap().as_str(),
        "pkg:oci/static@sha256:244fd47e07d10?tag=latest",
    );
    assert_eq!(
        Purl::oci_tag("debian", "sha256:244fd47e07d10", "bullseye").unwrap().as_str(),
        "pkg:oci/debian@sha256:244fd47e07d10?tag=bullseye",
    );
}

#[test]
fn rejects_missing_components() {
    assert_eq!(Purl::cargo("", "1.0.0"), Err(PurlError::EmptyName));
    assert_eq!(Purl::cargo("serde", ""), Err(PurlError::EmptyVersion));
}

#[test]
fn reports_its_type() {
    assert_eq!(Purl::npm("@a/b", "1.0.0").unwrap().purl_type(), "npm");
    assert_eq!(Purl::from_string_unchecked("pkg:cargo/x@1".to_owned()).purl_type(), "cargo");
}

#[test]
fn accepts_canonical_purls() {
    for purl in [
        Purl::npm("@example/widgets", "1.2.0-beta.1+build.5").unwrap(),
        Purl::npm("foobar", "12.3.1").unwrap(),
        Purl::npm_variant("@example/cli", "4.0.0", "darwin-arm64+darwin-x64").unwrap(),
        Purl::cargo("serde", "1.0.210").unwrap(),
        Purl::pypi_file("Django_Package", "1.11.1.dev1", "Django_Package-1.11.1.dev1.tar.gz")
            .unwrap(),
        Purl::from_string_unchecked("pkg:gem/rails@7.1.0".to_owned()),
    ] {
        purl.check_canonical().unwrap();
    }
}

#[test]
fn rejects_other_spellings_of_a_purl() {
    for text in [
        "garbage",
        "pkg:custom",
        "pkg:custom@1",
        "pkg:/widgets@1.2.0",
        "pkg:npm/",
        "pkg:npm/@example/widgets@1.2.0",
        "pkg:npm/%40example/widgets@1.2.0-beta.1+build.5",
        "pkg:npm/%2540example/widgets@1.2.0",
        "pkg:npm/example/widgets@1.2.0",
        "pkg:npm/widgets",
        "pkg:npm/widgets@1.2.0?arch=x64",
        "pkg:npm/widgets@1.2.0?variant=linux-x64+linux-x64-musl",
        "pkg:npm/widgets@1.2.0?variant=linux-x64&arch=x64",
        "pkg:npm/%77idgets@1.2.0",
        "pkg:cargo/serde@1.0.210#src",
        "pkg:cargo/ns/serde@1.0.210",
        "pkg:pypi/Django@1.11.1?file_name=Django-1.11.1.tar.gz",
        "pkg:pypi/django@1.11.1",
        "pkg:pypi/django@1.11.1?file_name=a.whl&arch=x86_64",
    ] {
        let purl = Purl::from_string_unchecked(text.to_owned());
        assert_eq!(
            purl.check_canonical(),
            Err(PurlError::NonCanonical { purl: text.to_owned() }),
            "{text}",
        );
    }
}

#[test]
fn spells_a_purl_canonically() {
    let npm_spelling = Purl::from_string_unchecked("pkg:npm/legacy!pkg@1.0.0".to_owned());
    assert_eq!(npm_spelling.canonical(), Some(Purl::npm("legacy!pkg", "1.0.0").unwrap()));
    assert_eq!(
        npm_spelling
            .canonical()
            .unwrap()
            .as_str(),
        "pkg:npm/legacy%21pkg@1.0.0",
    );
    assert_eq!(Purl::from_string_unchecked("not a purl".to_owned()).canonical(), None);
    let vcs = "pkg:npm/legacy!pkg@1.0.0?vcs_url=git%2Bhttps://example.com/legacy.git";
    assert_eq!(Purl::from_string_unchecked(vcs.to_owned()).canonical(), None);
}
