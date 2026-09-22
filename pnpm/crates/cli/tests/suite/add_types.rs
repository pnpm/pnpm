use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::{Value, json};
use std::{fmt::Write as _, fs, path::Path, process::Command};

fn serve_package(server: &mut mockito::Server, name: &str, extra: &Value) -> Vec<mockito::Mock> {
    let (version, packument) = package_metadata(server, name, extra);
    let encoded = name.replace('/', "%2f");
    vec![
        server
            .mock("GET", mockito::Matcher::Regex(format!("(?i)^/{encoded}$")))
            .with_header("content-type", "application/json")
            .with_body(packument.to_string())
            .expect_at_least(0)
            .create(),
        server
            .mock("GET", mockito::Matcher::Regex(format!("(?i)^/{encoded}/latest$")))
            .with_header("content-type", "application/json")
            .with_body(version.to_string())
            .expect_at_least(0)
            .create(),
    ]
}

fn package_metadata(server: &mockito::Server, name: &str, extra: &Value) -> (Value, Value) {
    let tarball = pnpm_testing_utils::fixtures::minimal_tarball(name, "1.0.0");
    let integrity = ssri::Integrity::from(tarball.as_slice()).to_string();
    let mut version = json!({
        "name": name, "version": "1.0.0",
        "dist": {"tarball": format!("{}/{name}/-/package.tgz", server.url()), "integrity": integrity}
    });
    version
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    let packument = json!({
        "name": name, "dist-tags": {"latest": "1.0.0"},
        "versions": {"1.0.0": version},
        "time": {"1.0.0": "2020-01-01T00:00:00.000Z"}
    });
    (version, packument)
}

fn setup(workspace: &Path, registry: &str) {
    fs::write(workspace.join("package.json"), "{}").unwrap();
    fs::write(workspace.join(".npmrc"), format!("registry={registry}/\n")).unwrap();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "minimumReleaseAge: 0\nstoreDir: .store\ncacheDir: .cache\nfetchRetries: 0\n",
    )
    .unwrap();
}

fn manifest(workspace: &Path) -> Value {
    serde_json::from_slice(&fs::read(workspace.join("package.json")).unwrap()).unwrap()
}

fn add(workspace: &Path, args: &[&str]) -> Command {
    Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(workspace)
        .with_args(["add", "--lockfile-only", "--ignore-scripts"])
        .with_args(args)
}

#[test]
fn saves_types_as_dev_dependencies_with_each_save_target() {
    for flag in ["--save-prod", "--save-dev", "--save-optional", "--save-peer"] {
        let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
        let mut server = mockito::Server::new();
        let _package = serve_package(&mut server, "example", &json!({}));
        let _types = serve_package(&mut server, "@types/example", &json!({}));
        setup(&workspace, &server.url());
        add(&workspace, &["example", "--save-types", flag]).assert().success();
        let result = manifest(&workspace);
        assert_eq!(result["devDependencies"]["@types/example"], "^1.0.0", "{flag}: {result}");
        let group = match flag {
            "--save-dev" | "--save-peer" => "devDependencies",
            "--save-optional" => "optionalDependencies",
            _ => "dependencies",
        };
        assert_eq!(result[group]["example"], "^1.0.0");
        let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).unwrap();
        assert!(lockfile.contains("'@types/example':"), "{lockfile}");
        drop(root);
    }
}

#[test]
fn maps_scoped_packages_and_npm_aliases_to_types() {
    for (selector, alias, types_alias, types_spec) in [
        ("@scope/example", "@scope/example", "@types/scope__example", "1.0.0"),
        (
            "renamed@npm:@scope/example@1",
            "renamed",
            "@types/renamed",
            "npm:@types/scope__example@1.0.0",
        ),
    ] {
        let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
        let mut server = mockito::Server::new();
        let _package = serve_package(&mut server, "@scope/example", &json!({}));
        let _types = serve_package(&mut server, "@types/scope__example", &json!({}));
        setup(&workspace, &server.url());
        add(&workspace, &[selector, "--save-types", "--save-exact"]).assert().success();
        let result = manifest(&workspace);
        assert_eq!(result["devDependencies"][types_alias], types_spec);
        assert!(result["dependencies"][alias].is_string(), "{result}");
        drop(root);
    }
}

#[test]
fn skips_bundled_types_and_missing_types_packages() {
    for extra in [
        json!({"types": "index.d.ts"}),
        json!({"typings": "index.d.ts"}),
        json!({"exports": {"types": ["./index.d.ts"]}}),
        json!({"exports": {"types": {"default": "./index.d.ts"}}}),
        json!({"exports": {".": {"import": {"types": "./index.d.mts"}}}}),
        json!({}),
    ] {
        let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
        let mut server = mockito::Server::new();
        let _package = serve_package(&mut server, "example", &extra);
        let missing = server
            .mock("GET", mockito::Matcher::Regex("(?i)^/@types%2fexample$".to_string()))
            .with_status(404)
            .expect(usize::from(extra == json!({})))
            .create();
        setup(&workspace, &server.url());
        add(&workspace, &["example", "--save-types"]).assert().success();
        assert_eq!(manifest(&workspace)["devDependencies"], Value::Null);
        missing.assert();
        drop(root);
    }
}

#[test]
fn retains_existing_types_and_honors_explicit_type_selectors() {
    for existing in [true, false] {
        let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
        let mut server = mockito::Server::new();
        let _package = serve_package(&mut server, "example", &json!({}));
        let _types = serve_package(&mut server, "@types/example", &json!({}));
        setup(&workspace, &server.url());
        if existing {
            fs::write(
                workspace.join("package.json"),
                json!({"dependencies": {"@types/example": "~1.0.0"}}).to_string(),
            )
            .unwrap();
        }
        let mut args = vec!["example", "--save-types"];
        if !existing {
            args.push("@types/example@~1.0.0");
        }
        add(&workspace, &args).assert().success();
        let result = manifest(&workspace);
        assert_eq!(result["dependencies"]["@types/example"], "~1.0.0");
        assert_eq!(result["devDependencies"], Value::Null);
        drop(root);
    }
}

#[test]
fn save_types_configuration_can_be_overridden() {
    for disabled in [false, true] {
        let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
        let mut server = mockito::Server::new();
        let _package = serve_package(&mut server, "example", &json!({}));
        let _types = serve_package(&mut server, "@types/example", &json!({}));
        setup(&workspace, &server.url());
        fs::write(workspace.join("pnpm-workspace.yaml"), "minimumReleaseAge: 0\nstoreDir: .store\ncacheDir: .cache\nfetchRetries: 0\nsaveTypes: true\n")
            .unwrap();
        let mut args = vec!["example"];
        if disabled {
            args.push("--no-save-types");
        }
        add(&workspace, &args).assert().success();
        assert_eq!(
            manifest(&workspace)["devDependencies"]["@types/example"],
            if disabled { Value::Null } else { json!("^1.0.0") },
        );
        drop(root);
    }
}

#[test]
fn types_registry_authentication_errors_are_not_ignored() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let _package = serve_package(&mut server, "example", &json!({}));
    let unauthorized = server
        .mock("GET", mockito::Matcher::Regex("(?i)^/@types%2fexample$".to_string()))
        .with_status(401)
        .create();
    setup(&workspace, &server.url());
    add(&workspace, &["example", "--save-types"]).assert().failure();
    unauthorized.assert();
    assert_eq!(manifest(&workspace), json!({}));
    drop(root);
}

#[test]
fn installs_types_in_selected_workspace_projects_and_catalog() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let _package = serve_package(&mut server, "example", &json!({}));
    let _types = serve_package(&mut server, "@types/example", &json!({}));
    let tarballs: Vec<_> = ["example", "@types/example"]
        .into_iter()
        .map(|name| {
            server
                .mock("GET", format!("/{name}/-/package.tgz").as_str())
                .with_body(pnpm_testing_utils::fixtures::minimal_tarball(name, "1.0.0"))
                .expect_at_least(1)
                .create()
        })
        .collect();
    setup(&workspace, &server.url());
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - 'packages/*'\nminimumReleaseAge: 0\nstoreDir: .store\ncacheDir: .cache\nsaveTypes: true\n",
    )
    .unwrap();
    for name in ["app-one", "app-two", "untouched"] {
        let directory = workspace.join("packages").join(name);
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("package.json"),
            json!({"name": name, "version": "1.0.0"}).to_string(),
        )
        .unwrap();
    }
    Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(&workspace)
        .with_args(["--filter", "app-*", "add", "example", "--save-catalog", "--ignore-scripts"])
        .assert()
        .success();
    for name in ["app-one", "app-two"] {
        let directory = workspace.join("packages").join(name);
        let result = manifest(&directory);
        assert_eq!(result["dependencies"]["example"], "catalog:");
        assert_eq!(result["devDependencies"]["@types/example"], "catalog:");
        assert!(
            directory.join("node_modules/@types/example/package.json").is_file(),
            "{directory:?}",
        );
    }
    assert_eq!(manifest(&workspace.join("packages/untouched"))["dependencies"], Value::Null);
    for tarball in tarballs {
        tarball.assert();
    }
    drop(root);
}

#[test]
fn save_types_environment_setting_and_cli_override() {
    for disabled in [false, true] {
        let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
        let mut server = mockito::Server::new();
        let _package = serve_package(&mut server, "example", &json!({}));
        let _types = serve_package(&mut server, "@types/example", &json!({}));
        setup(&workspace, &server.url());
        let mut command = add(&workspace, &["example"]);
        command.env("PNPM_CONFIG_SAVE_TYPES", "true");
        if disabled {
            command.arg("--no-save-types");
        }
        command.assert().success();
        assert_eq!(
            manifest(&workspace)["devDependencies"]["@types/example"],
            if disabled { Value::Null } else { json!("^1.0.0") },
        );
        drop(root);
    }
}

#[test]
fn local_packages_do_not_trigger_a_registry_types_lookup() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    setup(&workspace, &server.url());
    fs::create_dir(workspace.join("local")).unwrap();
    fs::write(
        workspace.join("local/package.json"),
        json!({"name": "private-local", "version": "1.0.0"}).to_string(),
    )
    .unwrap();
    let lookup = server
        .mock("GET", mockito::Matcher::Any)
        .expect(0)
        .create();
    add(&workspace, &["./local", "--save-types"]).assert().success();
    assert_eq!(manifest(&workspace)["dependencies"]["private-local"], "link:local");
    lookup.assert();
    drop(root);
}

#[test]
fn inspects_the_requested_version_for_bundled_types() {
    for (latest_version, initial_selector, alias, specifier, expected_types, expected_version) in [
        ("2.0.0", None, "example", "^1.0.0", "^1.0.0", "1.0.0"),
        ("1.1.0", Some("example@1.0.0"), "example", "^1.0.0", "^1.0.0", "1.0.0"),
        (
            "1.1.0",
            Some("renamed@npm:example@1.0.0"),
            "renamed",
            "npm:example@^1.0.0",
            "npm:@types/example@^1.0.0",
            "example@1.0.0",
        ),
    ] {
        let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
        let mut server = mockito::Server::new();
        let _types = serve_package(&mut server, "@types/example", &json!({}));
        let old_tarball = pnpm_testing_utils::fixtures::minimal_tarball("example", "1.0.0");
        let old = json!({"name": "example", "version": "1.0.0", "dist": {
            "tarball": format!("{}/example/-/old.tgz", server.url()),
            "integrity": ssri::Integrity::from(old_tarball.as_slice()).to_string()
        }});
        let mut latest = old.clone();
        latest["version"] = json!(latest_version);
        latest["types"] = json!("index.d.ts");
        let package = server
            .mock("GET", "/example")
            .with_header("content-type", "application/json")
            .with_body(
                json!({"name": "example", "dist-tags": {"latest": latest_version},
            "versions": {"1.0.0": old, (latest_version): latest}})
                .to_string(),
            )
            .expect_at_least(1)
            .create();
        setup(&workspace, &server.url());
        if let Some(selector) = initial_selector {
            add(&workspace, &[selector, "--save-exact"]).assert().success();
            fs::write(
                workspace.join("package.json"),
                json!({"dependencies": {(alias): specifier}}).to_string(),
            )
            .unwrap();
            add(&workspace, &[alias, "--save-types"]).assert().success();
        } else {
            add(&workspace, &["example@1", "--save-types"]).assert().success();
        }
        assert_eq!(
            manifest(&workspace)["devDependencies"][format!("@types/{alias}")],
            expected_types,
        );
        let lockfile =
            serde_json::to_value(crate::_utils::read_lockfile(&workspace.join("pnpm-lock.yaml")))
                .unwrap();
        assert_eq!(lockfile["importers"]["."]["dependencies"][alias]["version"], expected_version);
        package.assert();
        drop(root);
    }
}

#[test]
fn does_not_fetch_types_unless_enabled() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let _package = serve_package(&mut server, "example", &json!({}));
    let types = server
        .mock("GET", mockito::Matcher::Regex("(?i)^/@types%2fexample.*".to_string()))
        .expect(0)
        .create();
    setup(&workspace, &server.url());
    add(&workspace, &["example"]).assert().success();
    assert_eq!(manifest(&workspace)["devDependencies"], Value::Null);
    types.assert();
    drop(root);
}

#[test]
fn reuses_an_existing_types_catalog_entry() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let _package = serve_package(&mut server, "example", &json!({}));
    let _types = serve_package(&mut server, "@types/example", &json!({}));
    setup(&workspace, &server.url());
    fs::write(workspace.join("pnpm-workspace.yaml"), "minimumReleaseAge: 0\nstoreDir: .store\ncacheDir: .cache\ncatalogMode: strict\ncatalog:\n  '@types/example': '1.0.0'\n").unwrap();
    add(&workspace, &["example", "--save-types"]).assert().success();
    assert_eq!(manifest(&workspace)["devDependencies"]["@types/example"], "catalog:");
    let yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml")).unwrap();
    assert!(yaml.contains("'1.0.0'"), "{yaml}");
    drop(root);
}

#[test]
fn cross_registry_types_require_an_explicit_types_registry() {
    for explicit_types_registry in [false, true] {
        let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
        let mut private = mockito::Server::new();
        let mut public = mockito::Server::new();
        let _package = serve_package(&mut private, "@private/example", &json!({}));
        let _types = serve_package(&mut public, "@types/private__example", &json!({}));
        setup(&workspace, &public.url());
        let mut npmrc =
            format!("registry={}/\n@private:registry={}/\n", public.url(), private.url());
        if explicit_types_registry {
            writeln!(npmrc, "@types:registry={}/", public.url()).unwrap();
        }
        fs::write(workspace.join(".npmrc"), npmrc).unwrap();
        let forbidden_lookup = (!explicit_types_registry).then(|| {
            public
                .mock("GET", mockito::Matcher::Any)
                .expect(0)
                .create()
        });
        add(&workspace, &["@private/example", "--save-types"]).assert().success();
        let expected = if explicit_types_registry { json!("^1.0.0") } else { Value::Null };
        assert_eq!(manifest(&workspace)["devDependencies"]["@types/private__example"], expected);
        if let Some(lookup) = forbidden_lookup {
            lookup.assert();
        }
        drop(root);
    }
}

#[test]
fn bulk_add_fetches_only_required_metadata_formats() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let mut full_metadata = Vec::new();
    let mut abbreviated_metadata = Vec::new();
    for name in ["first", "second", "@types/first", "@types/second"] {
        let (_, packument) = package_metadata(&server, name, &json!({}));
        let encoded = name.replace('/', "%2f");
        let types_package = name.starts_with("@types/");
        full_metadata.push(
            server
                .mock("GET", mockito::Matcher::Regex(format!("(?i)^/{encoded}$")))
                .match_header("accept", "application/json; q=1.0, */*")
                .with_header("content-type", "application/json")
                .with_body(packument.to_string())
                .expect(usize::from(!types_package))
                .create(),
        );
        abbreviated_metadata.push(
            server
                .mock("GET", mockito::Matcher::Regex(format!("(?i)^/{encoded}$")))
                .match_header(
                    "accept",
                    "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*",
                )
                .with_header("content-type", "application/json")
                .with_body(packument.to_string())
                .expect(1 + usize::from(types_package))
                .create(),
        );
    }
    setup(&workspace, &server.url());
    add(&workspace, &["first", "second", "--save-types"]).assert().success();
    let result = manifest(&workspace);
    assert_eq!(result["devDependencies"]["@types/first"], "^1.0.0");
    assert_eq!(result["devDependencies"]["@types/second"], "^1.0.0");
    for mock in full_metadata.into_iter().chain(abbreviated_metadata) {
        mock.assert();
    }
    drop(root);
}

#[test]
fn an_alias_needs_its_own_types_import_name() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let _package = serve_package(&mut server, "example", &json!({}));
    let _types = serve_package(&mut server, "@types/example", &json!({}));
    setup(&workspace, &server.url());
    fs::write(
        workspace.join("package.json"),
        json!({"devDependencies": {"@types/example": "1.0.0"}}).to_string(),
    )
    .unwrap();
    add(&workspace, &["renamed@npm:example", "--save-types"]).assert().success();
    let result = manifest(&workspace);
    assert_eq!(result["devDependencies"]["@types/example"], "1.0.0");
    assert_eq!(result["devDependencies"]["@types/renamed"], "npm:@types/example@^1.0.0");
    drop(root);
}

#[test]
fn jsr_sources_and_direct_jsr_registry_packages_use_their_own_metadata_policy() {
    for (selector, direct) in [("@jsr/scope__example", true), ("jsr:@scope/example", false)] {
        let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
        let mut server = mockito::Server::new();
        let (_, full) =
            package_metadata(&server, "@jsr/scope__example", &json!({"types": "index.d.ts"}));
        let (_, abbreviated) = package_metadata(&server, "@jsr/scope__example", &json!({}));
        let mut requests = Vec::new();
        for (accept, body, count) in [
            ("application/json; q=1.0, */*", full, usize::from(direct)),
            (
                "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*",
                abbreviated,
                2 - usize::from(direct),
            ),
        ] {
            requests.push(
                server
                    .mock(
                        "GET",
                        mockito::Matcher::Regex("(?i)^/@jsr%2fscope__example$".to_string()),
                    )
                    .match_header("accept", accept)
                    .with_header("content-type", "application/json")
                    .with_body(body.to_string())
                    .expect(count)
                    .create(),
            );
        }
        requests.push(
            server
                .mock("GET", mockito::Matcher::Regex("(?i)^/@types".to_string()))
                .with_status(404)
                .expect(0)
                .create(),
        );
        setup(&workspace, &server.url());
        fs::write(
            workspace.join(".npmrc"),
            format!("registry={0}/\n@jsr:registry={0}/\n", server.url()),
        )
        .unwrap();
        add(&workspace, &[selector, "--save-types"]).assert().success();
        assert_eq!(manifest(&workspace)["devDependencies"], Value::Null);
        for request in requests {
            request.assert();
        }
        drop(root);
    }
}
