use super::{CargoDependencyKind, upsert_dependency};

#[test]
fn adds_a_dependency_without_reformatting_the_manifest() {
    let manifest =
        "[package]\nname = \"app\"\n\n[dependencies]\n# retained\n\n[features]\ndefault = []\n";
    let updated =
        upsert_dependency(manifest, CargoDependencyKind::Normal.table(), "serde", "1.0.228")
            .unwrap();

    assert_eq!(
        updated,
        "[package]\nname = \"app\"\n\n[dependencies]\n# retained\n\nserde = \"1.0.228\"\n[features]\ndefault = []\n",
    );
}

#[test]
fn creates_the_selected_dependency_table() {
    let updated = upsert_dependency(
        "[package]\nname = \"app\"\n",
        CargoDependencyKind::Development.table(),
        "insta",
        "1",
    )
    .unwrap();

    assert_eq!(updated, "[package]\nname = \"app\"\n\n[dev-dependencies]\ninsta = \"1\"\n");
}

#[test]
fn updates_strings_and_inline_tables_without_losing_formatting() {
    let manifest = "[dependencies]\nserde = '1' # keep\ntokio = { features = [\"version\"], version = \"1\" }\n";
    let updated = upsert_dependency(manifest, "dependencies", "serde", "2").unwrap();
    let updated = upsert_dependency(&updated, "dependencies", "tokio", "~1.43").unwrap();

    assert_eq!(
        updated,
        "[dependencies]\nserde = \"2\" # keep\ntokio = { features = [\"version\"], version = \"~1.43\" }\n",
    );
}
