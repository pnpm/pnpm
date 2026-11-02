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
        Purl::oci_tag("ghcr.io/debian", "sha256:244fd47e07d10", "bullseye").unwrap().as_str(),
        "pkg:oci/debian@sha256:244fd47e07d10?repository_url=ghcr.io%2Fdebian&tag=bullseye",
    );
    assert_eq!(
        Purl::oci_tag("gcr.io/distroless/static", "SHA256:244FD47E07D10", "latest")
            .unwrap()
            .as_str(),
        "pkg:oci/static@sha256:244fd47e07d10?repository_url=gcr.io%2Fdistroless%2Fstatic&tag=latest",
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
