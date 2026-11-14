use crate::{ProjectSnapshot, importer_link_target};

fn importer(source: &str) -> ProjectSnapshot {
    serde_saphyr::from_str(source).expect("parse importer")
}

#[test]
fn link_target_follows_publish_directory_unless_link_directory_is_false() {
    assert_eq!(importer_link_target("packages/dep", &importer("{}")), "packages/dep");
    assert_eq!(
        importer_link_target("packages/dep", &importer("publishDirectory: ./dist")),
        "packages/dep/dist",
    );
    assert_eq!(
        importer_link_target(
            "packages/dep",
            &importer("publishDirectory: dist\nlinkDirectory: false"),
        ),
        "packages/dep",
    );
    assert_eq!(importer_link_target(".", &importer("publishDirectory: ../out")), "../out");
}
