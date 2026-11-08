use super::{GivenIntegrity, archives_of, check_recursive, parse};
use crate::cli_args::publish::recursive::publish_eligible;
use pnpm_pack::variants::VariantDeclaration;
use pnpm_publish::VariantArchive;
use serde_json::json;

fn declaration() -> VariantDeclaration {
    VariantDeclaration::read(&json!({
        "name": "example-cli",
        "version": "4.0.0",
        "publishConfig": {
            "variants": [
                { "targets": ["any"] },
                { "targets": ["linux-x64", "linux-x64-musl"] },
                { "targets": ["win32-x64"] },
            ],
        },
    }))
    .unwrap()
    .unwrap()
}

fn values(values: &[&str]) -> Vec<GivenIntegrity> {
    parse(
        &values
            .iter()
            .map(|value| (*value).to_string())
            .collect::<Vec<_>>(),
    )
    .unwrap()
}

#[test]
fn parses_a_value_at_its_first_equals_and_its_package_at_the_last_tilde() {
    assert_eq!(
        values(&["@scope/a~b~linux-x64=sha512-x=="]),
        [GivenIntegrity {
            package: Some("@scope/a~b".into()),
            target: "linux-x64".into(),
            integrity: "sha512-x==".into(),
        }],
    );
    for invalid in ["", "linux-x64", "linux-x64=", "=sha512-x"] {
        assert!(parse(&[invalid.to_string()]).is_err(), "{invalid:?}");
    }
}

#[test]
fn takes_every_archive_from_exactly_one_value_naming_it_by_any_target() {
    let given =
        values(&["any=sha512-a", "linux-x64-musl=sha512-l", "example-cli~win32-x64=sha512-w"]);
    let (any, variants) = archives_of("example-cli", &declaration(), &given).unwrap().unwrap();
    assert_eq!(any.unwrap().integrity, "sha512-a");
    let tags: Vec<(&str, &str)> = variants
        .iter()
        .map(|variant| {
            let VariantArchive::Staged(staged) = &variant.archive else { panic!() };
            (variant.tag.as_str(), staged.integrity.as_str())
        })
        .collect();
    assert_eq!(tags, [("linux-x64+linux-x64-musl", "sha512-l"), ("win32-x64", "sha512-w")]);
    let others = values(&["other~win32-x64=sha512-w"]);
    assert!(archives_of("example-cli", &declaration(), &others).unwrap().is_none());
}

#[test]
fn refuses_a_missing_unknown_or_twice_given_archive() {
    for (case, given, expected) in [
        ("missing", vec!["any=sha512-a", "linux-x64=sha512-l"], "gives no archive for win32-x64"),
        ("unknown", vec!["any=sha512-a", "darwin-arm64=sha512-d"], "darwin-arm64"),
        (
            "twice",
            vec!["linux-x64=sha512-l", "linux-x64-musl=sha512-m"],
            "names the archive for linux-x64, linux-x64-musl of example-cli twice",
        ),
    ] {
        let error = archives_of("example-cli", &declaration(), &values(&given)).unwrap_err();
        assert!(error.to_string().contains(expected), "{case}: {error}");
    }
}

#[test]
fn a_recursive_publish_takes_values_of_its_selected_packages() {
    let packages = ["example-cli", "example-addon"];
    check_recursive(&values(&["example-cli~linux-x64=sha512-a"]), &packages).unwrap();
    let unnamed = check_recursive(&values(&["linux-x64=sha512-a"]), &packages).unwrap_err();
    assert!(unnamed.to_string().contains("names no package"), "{unnamed}");
    let mistyped =
        check_recursive(&values(&["example-clj~linux-x64=sha512-a"]), &packages).unwrap_err();
    assert!(mistyped.to_string().contains("example-clj"), "{mistyped}");
    let twice = check_recursive(
        &values(&["example-cli~linux-x64=sha512-a"]),
        &["example-cli", "example-cli"],
    )
    .unwrap_err();
    assert!(twice.to_string().contains("several selected projects"), "{twice}");
}

#[test]
fn a_renamed_package_is_named_by_its_published_name() {
    let renamed = json!({
        "name": "example-cli-src",
        "version": "4.0.0",
        "publishConfig": { "name": "example-cli" },
    });
    let private_root = json!({
        "name": "workspace-root",
        "version": "0.0.0",
        "private": true,
        "publishConfig": { "name": "example-cli" },
    });
    assert_eq!(publish_eligible(&private_root), None, "a private project publishes nothing");
    let packages = [publish_eligible(&renamed).unwrap().0];
    check_recursive(&values(&["example-cli~linux-x64=sha512-a"]), &packages).unwrap();
    check_recursive(&values(&["example-cli-src~linux-x64=sha512-a"]), &packages).unwrap_err();
}
