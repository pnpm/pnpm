use super::RemoveDependencyOptions;
use pnpm_package_manifest::DependencyGroup;
use pretty_assertions::assert_eq;

#[test]
fn dependency_options_to_save_type() {
    use DependencyGroup::{
        Dev,
        Optional,
        Prod,
    };
    let save_type = |opts: RemoveDependencyOptions| opts.save_type();

    assert_eq!(
        save_type(RemoveDependencyOptions {
            save_prod: false,
            save_dev: false,
            save_optional: false
        }),
        None,
    );

    assert_eq!(
        save_type(RemoveDependencyOptions {
            save_prod: true,
            save_dev: false,
            save_optional: false
        }),
        Some(Prod),
    );

    assert_eq!(
        save_type(RemoveDependencyOptions {
            save_prod: false,
            save_dev: true,
            save_optional: false
        }),
        Some(Dev),
    );

    assert_eq!(
        save_type(RemoveDependencyOptions {
            save_prod: false,
            save_dev: false,
            save_optional: true
        }),
        Some(Optional),
    );

    assert_eq!(
        save_type(RemoveDependencyOptions { save_prod: true, save_dev: true, save_optional: true }),
        Some(Dev),
    );
}

#[test]
fn npm_package_urls_remove_by_name_and_preserve_ordinary_patterns() {
    use clap::Parser;

    #[derive(Parser)]
    struct Cli {
        #[clap(flatten)]
        args: super::RemoveArgs,
    }
    let mut cli = Cli::parse_from(["pnpm", "pkg:npm/%40scope/example@1.2.3", "foo-*", "!foo-test"]);
    assert!(
        cli.args
            .take_ecosystem_packages(&pnpm_config::Config::default())
            .unwrap()
            .is_empty(),
    );
    assert_eq!(cli.args.package_names, ["@scope/example", "foo-*", "!foo-test"]);
}
