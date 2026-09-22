use super::{
    DependencySelection,
    Manifest,
};
use pnpm_config::Config;

const PROJECT: &str = "[project]\nname = 'app'\nversion = '1.0'\ndependencies = ['base']\n[project.optional-dependencies]\ncli = ['alpha']\nweb = ['beta']\ndev_tools = ['gamma']\n[dependency-groups]\ndev = ['pytest']\ntest = [{include-group = 'dev'}, 'coverage']\n";

fn requirements(
    manifest: &Manifest,
    config: &Config,
    selection: DependencySelection,
) -> Vec<String> {
    manifest
        .requirements(config, selection)
        .unwrap()
        .into_iter()
        .map(|requirement| requirement.to_string())
        .collect()
}

#[test]
fn workspace_defaults_select_only_names_the_project_defines() {
    let manifest = Manifest::parse(PROJECT).unwrap();
    let mut config = Config::new();
    config.python.extras = vec!["cli".into(), "missing".into(), "dev-tools".into()];
    config.python.groups = vec!["test".into(), "missing".into()];
    assert_eq!(
        requirements(&manifest, &config, DependencySelection::ALL),
        ["base", "alpha", "gamma", "pytest", "coverage"],
    );
}

#[test]
fn project_selections_override_each_workspace_default_independently() {
    let mut config = Config::new();
    config.python.extras = vec!["cli".into()];
    config.python.groups = vec!["test".into()];
    let manifest =
        Manifest::parse(&format!("{PROJECT}\n[tool.pnpm.python]\nextras = ['web']\n")).unwrap();
    assert_eq!(
        requirements(&manifest, &config, DependencySelection::ALL),
        ["base", "beta", "pytest", "coverage"],
    );
    let manifest =
        Manifest::parse(&format!("{PROJECT}\n[tool.pnpm.python]\ngroups = ['dev']\n")).unwrap();
    assert_eq!(
        requirements(&manifest, &config, DependencySelection::ALL),
        ["base", "alpha", "pytest"],
    );
}

#[test]
fn empty_project_selections_disable_defaults_and_keep_install_projections() {
    let mut config = Config::new();
    config.python.extras = vec!["cli".into()];
    config.python.groups = vec!["test".into()];
    let manifest =
        Manifest::parse(&format!("{PROJECT}\n[tool.pnpm.python]\nextras = []\ngroups = []\n"))
            .unwrap();
    assert_eq!(requirements(&manifest, &config, DependencySelection::ALL), ["base"]);
    let manifest = Manifest::parse(&format!(
        "{PROJECT}\n[tool.pnpm.python]\nextras = ['web']\ngroups = ['test']\n",
    ))
    .unwrap();
    assert_eq!(
        requirements(
            &manifest,
            &config,
            DependencySelection { production: true, development: false }
        ),
        ["base", "beta"],
    );
    assert_eq!(
        requirements(
            &manifest,
            &config,
            DependencySelection { production: false, development: true }
        ),
        ["pytest", "coverage"],
    );
}

#[test]
fn missing_explicit_project_selections_are_errors() {
    for (setting, error) in
        [("extras", "unknown Python project extra"), ("groups", "unknown Python dependency group")]
    {
        let manifest =
            Manifest::parse(&format!("{PROJECT}\n[tool.pnpm.python]\n{setting} = ['missing']\n"))
                .unwrap();
        let result = manifest
            .requirements(&Config::new(), DependencySelection::ALL)
            .unwrap_err()
            .to_string();
        eprintln!("{result}");
        assert!(result.contains(error));
    }
}

#[test]
fn missing_or_cyclic_included_groups_are_still_errors() {
    for (entry, error) in
        [("missing", "unknown Python dependency group"), ("dev", "cyclic Python dependency group")]
    {
        let manifest = Manifest::parse(&PROJECT.replace(
            "dev = ['pytest']",
            &format!("dev = [{{include-group = '{entry}'}}]"),
        ))
        .unwrap();
        let result = manifest
            .requirements(&Config::new(), DependencySelection::ALL)
            .unwrap_err()
            .to_string();
        eprintln!("{result}");
        assert!(result.contains(error));
    }
}

#[test]
fn unknown_project_selection_settings_are_rejected() {
    let result = Manifest::parse(&format!("{PROJECT}\n[tool.pnpm.python]\ngropus = ['dev']\n"))
        .err()
        .unwrap()
        .to_string();
    eprintln!("{result}");
    assert!(result.contains("unknown field"));
}

#[test]
fn explicit_selections_are_validated_before_install_projections() {
    for selection in [
        DependencySelection { production: true, development: false },
        DependencySelection { production: false, development: true },
    ] {
        for (setting, error) in [
            ("extras", "unknown Python project extra"),
            ("groups", "unknown Python dependency group"),
        ] {
            let manifest = Manifest::parse(&format!(
                "{PROJECT}\n[tool.pnpm.python]\n{setting} = ['missing']\n",
            ))
            .unwrap();
            let result = manifest
                .requirements(&Config::new(), selection)
                .unwrap_err()
                .to_string();
            eprintln!("{result}");
            assert!(result.contains(error));
        }
        for (included, error) in [
            ("missing", "unknown Python dependency group"),
            ("dev", "cyclic Python dependency group"),
        ] {
            let manifest = Manifest::parse(&format!(
                "{}\n[tool.pnpm.python]\ngroups = ['dev']\n",
                PROJECT.replace(
                    "dev = ['pytest']",
                    &format!("dev = [{{include-group = '{included}'}}]")
                ),
            ))
            .unwrap();
            let result = manifest
                .requirements(&Config::new(), selection)
                .unwrap_err()
                .to_string();
            eprintln!("{result}");
            assert!(result.contains(error));
        }
    }
}
