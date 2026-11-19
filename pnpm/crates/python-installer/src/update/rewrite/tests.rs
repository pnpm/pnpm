use std::{collections::BTreeSet, fs};

use pep508_rs::Requirement;
use pnpm_config::Config;

use super::{rewrite_manifest_text, rewrite_pyproject, rewrite_pyproject_many};
use crate::{PythonDependencyGroup, discover};

fn req(text: &str) -> Requirement {
    text.parse().unwrap()
}

#[test]
fn optional_dependencies_table_header_stays_untouched_when_updating_main_dependency_only() {
    let original = "\
[project]
name = \"my-app\"
version = \"0.1.0\"
requires-python = \">=3.10\"
dependencies = [\"beta>=1.0.0\"]

# Keep section comments and single quotes intact.
[project.optional-dependencies]
cli = ['alpha>=1.0.0']
";
    let updated = rewrite_manifest_text(original, &BTreeSet::new(), &mut |group, item| {
        if group == PythonDependencyGroup::Prod && item.name.as_ref() == "beta" {
            return Ok(Some(vec![req("beta>=1.5.0")]));
        }
        Ok(None)
    })
    .unwrap()
    .unwrap();
    let expected = "\
[project]
name = \"my-app\"
version = \"0.1.0\"
requires-python = \">=3.10\"
dependencies = [\"beta>=1.5.0\"]

# Keep section comments and single quotes intact.
[project.optional-dependencies]
cli = ['alpha>=1.0.0']
";
    eprintln!("UPDATED:\n{updated}\n");
    assert_eq!(updated, expected);
}

#[test]
fn optional_dependencies_table_header_updates_extra_and_preserves_comments_and_other_extras() {
    let original = "\
[project]
name = \"my-app\"
version = \"0.1.0\"
dependencies = [\"beta>=1.0.0\"]

[project.optional-dependencies]
# CLI tooling
cli = [
    # Primary CLI helper
    \"alpha>=1.0.0\",
]
test = ['pytest>=8.0.0']
";
    let updated = rewrite_manifest_text(original, &BTreeSet::new(), &mut |group, item| {
        if group == PythonDependencyGroup::Optional && item.name.as_ref() == "alpha" {
            return Ok(Some(vec![req("alpha>=2.0.0")]));
        }
        Ok(None)
    })
    .unwrap()
    .unwrap();
    let expected = "\
[project]
name = \"my-app\"
version = \"0.1.0\"
dependencies = [\"beta>=1.0.0\"]

[project.optional-dependencies]
# CLI tooling
cli = [
    # Primary CLI helper
    \"alpha>=2.0.0\",
]
test = ['pytest>=8.0.0']
";
    eprintln!("UPDATED:\n{updated}\n");
    assert_eq!(updated, expected);
}

#[test]
fn inline_optional_dependencies_table_updates_in_place() {
    let original = "\
[project]
name = \"my-app\"
version = \"0.1.0\"
optional-dependencies = { cli = ['alpha>=1.0.0'], test = [\"pytest==8.0.0\"] }
";
    let updated = rewrite_manifest_text(original, &BTreeSet::new(), &mut |group, item| {
        if group == PythonDependencyGroup::Optional && item.name.as_ref() == "pytest" {
            return Ok(Some(vec![req("pytest==8.2.0")]));
        }
        Ok(None)
    })
    .unwrap()
    .unwrap();
    let expected = "\
[project]
name = \"my-app\"
version = \"0.1.0\"
optional-dependencies = { cli = ['alpha>=1.0.0'], test = [\"pytest==8.2.0\"] }
";
    eprintln!("UPDATED:\n{updated}\n");
    assert_eq!(updated, expected);
}

#[test]
fn dotted_optional_dependencies_keys_update_in_place() {
    let original = "\
[project]
name = \"my-app\"
version = \"0.1.0\"
optional-dependencies.cli = [\"alpha>=1.0.0\"]
optional-dependencies.docs = [\"mkdocs>=1.5.0\"]
";
    let updated = rewrite_manifest_text(original, &BTreeSet::new(), &mut |group, item| {
        if group == PythonDependencyGroup::Optional && item.name.as_ref() == "alpha" {
            return Ok(Some(vec![req("alpha>=1.4.0")]));
        }
        Ok(None)
    })
    .unwrap()
    .unwrap();
    let expected = "\
[project]
name = \"my-app\"
version = \"0.1.0\"
optional-dependencies.cli = [\"alpha>=1.4.0\"]
optional-dependencies.docs = [\"mkdocs>=1.5.0\"]
";
    eprintln!("UPDATED:\n{updated}\n");
    assert_eq!(updated, expected);
}

#[test]
fn top_level_dotted_project_optional_dependencies_update_in_place() {
    let original = "\
project.name = \"my-app\"
project.version = \"0.1.0\"
project.optional-dependencies.cli = [\"alpha>=1.0.0\"]
";
    let updated = rewrite_manifest_text(original, &BTreeSet::new(), &mut |group, item| {
        if group == PythonDependencyGroup::Optional && item.name.as_ref() == "alpha" {
            return Ok(Some(vec![req("alpha>=2.0.0")]));
        }
        Ok(None)
    })
    .unwrap()
    .unwrap();
    let expected = "\
project.name = \"my-app\"
project.version = \"0.1.0\"
project.optional-dependencies.cli = [\"alpha>=2.0.0\"]
";
    eprintln!("UPDATED:\n{updated}\n");
    assert_eq!(updated, expected);
}

#[test]
fn dependency_groups_preserve_include_group_tables_across_representations() {
    let header_form = "\
[project]
name = \"my-app\"
version = \"0.1.0\"

[dependency-groups]
lint = [\"ruff>=0.4.0\"]
dev = [{ include-group = \"lint\" }, \"pytest>=8.0.0\"]
";
    let updated_header =
        rewrite_manifest_text(header_form, &BTreeSet::new(), &mut |group, item| {
            if group == PythonDependencyGroup::Dev && item.name.as_ref() == "pytest" {
                return Ok(Some(vec![req("pytest>=8.3.0")]));
            }
            Ok(None)
        })
        .unwrap()
        .unwrap();
    let expected_header = "\
[project]
name = \"my-app\"
version = \"0.1.0\"

[dependency-groups]
lint = [\"ruff>=0.4.0\"]
dev = [{ include-group = \"lint\" }, \"pytest>=8.3.0\"]
";
    eprintln!("UPDATED HEADER:\n{updated_header}\n");
    assert_eq!(updated_header, expected_header);

    let inline_form = "\
dependency-groups = { dev = [{ include-group = \"lint\" }, \"pytest>=8.0.0\"], lint = [\"ruff>=0.4.0\"] }

[project]
name = \"my-app\"
version = \"0.1.0\"
";
    let updated_inline =
        rewrite_manifest_text(inline_form, &BTreeSet::new(), &mut |group, item| {
            if group == PythonDependencyGroup::Dev && item.name.as_ref() == "pytest" {
                return Ok(Some(vec![req("pytest>=8.3.0")]));
            }
            Ok(None)
        })
        .unwrap()
        .unwrap();
    let expected_inline = "\
dependency-groups = { dev = [{ include-group = \"lint\" }, \"pytest>=8.3.0\"], lint = [\"ruff>=0.4.0\"] }

[project]
name = \"my-app\"
version = \"0.1.0\"
";
    eprintln!("UPDATED INLINE:\n{updated_inline}\n");
    assert_eq!(updated_inline, expected_inline);

    let dotted_form = "\
dependency-groups.lint = [\"ruff>=0.4.0\"]
dependency-groups.dev = [{ include-group = \"lint\" }, \"pytest>=8.0.0\"]

[project]
name = \"my-app\"
version = \"0.1.0\"
";
    let updated_dotted =
        rewrite_manifest_text(dotted_form, &BTreeSet::new(), &mut |group, item| {
            if group == PythonDependencyGroup::Dev && item.name.as_ref() == "pytest" {
                return Ok(Some(vec![req("pytest>=8.3.0")]));
            }
            Ok(None)
        })
        .unwrap()
        .unwrap();
    let expected_dotted = "\
dependency-groups.lint = [\"ruff>=0.4.0\"]
dependency-groups.dev = [{ include-group = \"lint\" }, \"pytest>=8.3.0\"]

[project]
name = \"my-app\"
version = \"0.1.0\"
";
    eprintln!("UPDATED DOTTED:\n{updated_dotted}\n");
    assert_eq!(updated_dotted, expected_dotted);
}

#[test]
fn multi_entry_expansion_and_empty_entry_removal_produce_valid_toml() {
    let original = "\
[project]
name = \"my-app\"
version = \"0.1.0\"

[project.optional-dependencies]
cli = [\"alpha>=1.0.0\", \"drop-me>=1.0.0\"]
platform = [\"beta>=1.0.0\"]
";
    let updated = rewrite_manifest_text(original, &BTreeSet::new(), &mut |_, item| match item
        .name
        .as_ref()
    {
        "drop-me" => Ok(Some(vec![])),
        "beta" => Ok(Some(vec![
            req("beta==1.0.0 ; sys_platform == 'linux'"),
            req("beta==2.0.0 ; sys_platform == 'win32'"),
        ])),
        _ => Ok(None),
    })
    .unwrap()
    .unwrap();
    let expected = "\
[project]
name = \"my-app\"
version = \"0.1.0\"

[project.optional-dependencies]
cli = [\"alpha>=1.0.0\"]
platform = [\"beta==1.0.0 ; sys_platform == 'linux'\", \"beta==2.0.0 ; sys_platform == 'win32'\"]
";
    eprintln!("UPDATED:\n{updated}\n");
    assert_eq!(updated, expected);
}

#[tokio::test]
async fn rewrite_pyproject_skips_non_index_sources_and_rewrites_section_extras_on_disk() {
    let temp = tempfile::tempdir().unwrap();
    let local_pkg = temp.path().join("local-pkg");
    fs::create_dir_all(&local_pkg).unwrap();
    fs::write(
        local_pkg.join("pyproject.toml"),
        "[project]\nname = \"local-pkg\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();

    let manifest_path = temp.path().join("pyproject.toml");
    let initial = "\
[project]
name = \"my-app\"
version = \"0.1.0\"
dependencies = [\"beta>=1.0.0\"]

[project.optional-dependencies]
cli = [\"alpha>=1.0.0\", \"local-pkg>=0.1.0\"]

[tool.uv.sources]
local-pkg = { path = \"local-pkg\" }
";
    fs::write(&manifest_path, initial).unwrap();

    let config = Config { workspace_dir: Some(temp.path().to_path_buf()), ..Config::default() };
    let discovery = discover(&config, temp.path(), std::slice::from_ref(&manifest_path))
        .await
        .unwrap();

    rewrite_pyproject_many(temp.path(), &discovery.workspace, |_, item| {
        Ok(Some(vec![req(&format!("{}==1.0.0", item.name))]))
    })
    .unwrap();
    let staged = fs::read_to_string(&manifest_path).unwrap();
    let expected_staged = "\
[project]
name = \"my-app\"
version = \"0.1.0\"
dependencies = [\"beta==1.0.0\"]

[project.optional-dependencies]
cli = [\"alpha==1.0.0\", \"local-pkg>=0.1.0\"]

[tool.uv.sources]
local-pkg = { path = \"local-pkg\" }
";
    eprintln!("STAGED:\n{staged}\n");
    assert_eq!(staged, expected_staged);

    rewrite_pyproject(temp.path(), &discovery.workspace, |group, item| {
        if group == PythonDependencyGroup::Optional && item.name.as_ref() == "alpha" {
            return Ok(Some(req("alpha>=1.2.0")));
        }
        Ok(None)
    })
    .unwrap();
    let finalized = fs::read_to_string(&manifest_path).unwrap();
    let expected_finalized = "\
[project]
name = \"my-app\"
version = \"0.1.0\"
dependencies = [\"beta==1.0.0\"]

[project.optional-dependencies]
cli = [\"alpha>=1.2.0\", \"local-pkg>=0.1.0\"]

[tool.uv.sources]
local-pkg = { path = \"local-pkg\" }
";
    eprintln!("FINALIZED:\n{finalized}\n");
    assert_eq!(finalized, expected_finalized);
}
