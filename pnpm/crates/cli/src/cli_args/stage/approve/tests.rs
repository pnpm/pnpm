use std::collections::HashSet;

use pretty_assertions::assert_eq;

use serde_json::json;

use super::{
    StageApprovalItem, StageError, WorkspaceApprovalOrder, parse_stage_ids,
    sort_items_for_approval, unavailable_dependencies,
};

fn item(id: &str, package_name: Option<&str>, version: Option<&str>) -> StageApprovalItem {
    StageApprovalItem {
        id: id.to_owned(),
        package_name: package_name.map(str::to_owned),
        version: version.map(str::to_owned),
        tag: None,
        created_at: None,
        actor: None,
    }
}

fn order(chunks: &[&[&str]], dependencies: &[(&str, &[&str])]) -> WorkspaceApprovalOrder {
    WorkspaceApprovalOrder {
        chunk_index_by_package_name: chunks
            .iter()
            .enumerate()
            .flat_map(|(chunk_index, chunk)| {
                chunk.iter().map(move |name| ((*name).to_owned(), chunk_index))
            })
            .collect(),
        dependency_names_by_package_name: dependencies
            .iter()
            .map(|(name, deps)| {
                ((*name).to_owned(), deps.iter().map(|dep| (*dep).to_owned()).collect())
            })
            .collect(),
    }
}

#[test]
fn workspace_dependencies_are_approved_before_their_dependents() {
    let items = vec![
        item("id-dependent", Some("dependent"), Some("1.0.0")),
        item("id-dependency", Some("dependency"), Some("1.0.0")),
    ];
    let order = order(&[&["dependency"], &["dependent"]], &[("dependent", &["dependency"])]);
    let sorted = sort_items_for_approval(items, Some(&order));
    assert_eq!(
        sorted.iter().map(|item| item.id.as_str()).collect::<Vec<_>>(),
        ["id-dependency", "id-dependent"],
    );
}

#[test]
fn packages_outside_the_workspace_keep_their_order_after_the_workspace_ones() {
    let items = vec![
        item("id-external", Some("external"), Some("1.0.0")),
        item("id-unlisted", None, None),
        item("id-dependency", Some("dependency"), Some("1.0.0")),
    ];
    let order = order(&[&["dependency"]], &[]);
    let sorted = sort_items_for_approval(items, Some(&order));
    assert_eq!(
        sorted.iter().map(|item| item.id.as_str()).collect::<Vec<_>>(),
        ["id-dependency", "id-external", "id-unlisted"],
    );
}

#[test]
fn selection_order_is_kept_outside_a_workspace() {
    let items = vec![
        item("id-dependent", Some("dependent"), Some("1.0.0")),
        item("id-dependency", Some("dependency"), Some("1.0.0")),
    ];
    let sorted = sort_items_for_approval(items, None);
    assert_eq!(
        sorted.iter().map(|item| item.id.as_str()).collect::<Vec<_>>(),
        ["id-dependent", "id-dependency"],
    );
}

#[test]
fn a_dependent_of_an_unpublished_package_is_blocked() {
    let order = order(&[&["dependency"], &["dependent"]], &[("dependent", &["dependency"])]);
    let unpublished: HashSet<String> = std::iter::once("dependency".to_owned()).collect();
    assert_eq!(
        unavailable_dependencies(
            &item("id", Some("dependent"), Some("1.0.0")),
            &unpublished,
            Some(&order)
        ),
        ["dependency"],
    );
    assert!(
        unavailable_dependencies(
            &item("id", Some("dependency"), Some("1.0.0")),
            &unpublished,
            Some(&order),
        )
        .is_empty(),
    );
}

#[test]
fn nothing_is_blocked_without_workspace_knowledge() {
    let unpublished: HashSet<String> = std::iter::once("dependency".to_owned()).collect();
    assert!(
        unavailable_dependencies(&item("id", Some("dependent"), Some("1.0.0")), &unpublished, None)
            .is_empty(),
    );
}

#[test]
fn stage_ids_are_validated_as_uuids() {
    let params = vec![
        "approve".to_owned(),
        "1de6f3db-2ed9-4d72-b3dd-8f0e2b474a2f".to_owned(),
        "2b8f1c14-4a0d-4a4a-9a2e-6c5a2f0a1b33".to_owned(),
    ];
    assert_eq!(parse_stage_ids(&params).unwrap(), params[1..]);
    assert!(parse_stage_ids(&["approve".to_owned()]).unwrap().is_empty());
    assert!(matches!(
        parse_stage_ids(&["approve".to_owned(), "not-a-uuid".to_owned()]),
        Err(StageError::InvalidStageId),
    ));
}

#[test]
fn a_repeated_stage_id_is_approved_once_whatever_its_spelling() {
    let params = vec![
        "approve".to_owned(),
        "1de6f3db-2ed9-4d72-b3dd-8f0e2b474a2f".to_owned(),
        "2b8f1c14-4a0d-4a4a-9a2e-6c5a2f0a1b33".to_owned(),
        "1DE6F3DB-2ED9-4D72-B3DD-8F0E2B474A2F".to_owned(),
    ];
    assert_eq!(parse_stage_ids(&params).unwrap(), params[1..3]);
}

#[test]
fn a_staged_version_is_named_by_its_package_and_falls_back_to_its_id() {
    let named = item("1de6f3db-2ed9-4d72-b3dd-8f0e2b474a2f", Some("foo"), Some("1.0.0"));
    assert_eq!(named.label(), "foo@1.0.0");
    assert_eq!(named.reference(), "foo@1.0.0 (1de6f3db-2ed9-4d72-b3dd-8f0e2b474a2f)");
    let unlisted = item("1de6f3db-2ed9-4d72-b3dd-8f0e2b474a2f", None, None);
    assert_eq!(unlisted.label(), "1de6f3db-2ed9-4d72-b3dd-8f0e2b474a2f");
    assert_eq!(unlisted.reference(), "1de6f3db-2ed9-4d72-b3dd-8f0e2b474a2f");
}

#[test]
fn a_listed_staged_version_is_stripped_of_terminal_control_characters() {
    let listed = StageApprovalItem::from_value(&json!({
        "id": "1de6f3db-2ed9-4d72-b3dd-8f0e2b474a2f",
        "packageName": "foo\u{1b}[2K\u{1b}[G",
        "version": "1.0.0",
        "actor": "zkochan\u{202e}",
    }))
    .expect("a staged version");
    assert_eq!(listed.label(), "foo[2K[G@1.0.0");
    assert_eq!(listed.choice(), "foo[2K[G@1.0.0 (by zkochan)");
}

#[test]
fn a_listed_staged_version_without_a_uuid_id_is_dropped() {
    assert!(
        StageApprovalItem::from_value(&json!({
            "id": "../../../-/npm/v1/tokens",
            "packageName": "foo",
            "version": "1.0.0",
        }))
        .is_none(),
    );
    // Sanitizing the id must not be what turns it into a UUID.
    assert!(
        StageApprovalItem::from_value(&json!({
            "id": "1de6f3db-2ed9-4d72-b3dd-8f0e2b474a2f\u{202e}",
            "packageName": "foo",
            "version": "1.0.0",
        }))
        .is_none(),
    );
}

#[test]
fn the_picker_shows_the_tag_and_who_staged_it() {
    let mut staged = item("id", Some("foo"), Some("1.0.0"));
    staged.tag = Some("next".to_owned());
    staged.actor = Some("zkochan".to_owned());
    assert_eq!(staged.choice(), "foo@1.0.0 (next, by zkochan)");
}
