use super::split_git_conflict;
use pretty_assertions::assert_eq;

#[test]
fn splits_multiple_conflicts_and_discards_the_diff3_parent() {
    let conflict = "common\n<<<<<<< HEAD\nours one\n||||||| parent\nbase one\n=======\ntheirs one\n>>>>>>> branch\nmiddle\n<<<<<<< HEAD\nours two\n=======\ntheirs two\n>>>>>>> branch\n";

    let (ours, theirs) = split_git_conflict(conflict).expect("git conflict markers");

    assert_eq!(ours, "common\nours one\nmiddle\nours two\n");
    assert_eq!(theirs, "common\ntheirs one\nmiddle\ntheirs two\n");
}

#[test]
fn returns_none_without_a_complete_conflict() {
    assert!(split_git_conflict("lockfileVersion: '9.0'\n").is_none());
    assert!(split_git_conflict("<<<<<<< HEAD\nours\n=======\ntheirs\n").is_none());
}
