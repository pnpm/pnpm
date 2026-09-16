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

#[test]
fn returns_none_when_a_later_conflict_is_unterminated() {
    let conflict = "<<<<<<< HEAD\nours one\n=======\ntheirs one\n>>>>>>> branch\n<<<<<<< HEAD\nours two\n=======\ntheirs two\n";

    assert!(split_git_conflict(conflict).is_none());
}

#[test]
fn returns_none_for_markers_git_would_not_have_written() {
    assert!(split_git_conflict(">>>>>>> branch\n").is_none());
    assert!(split_git_conflict("<<<<<<< HEAD\nours\n>>>>>>> branch\n").is_none());
    assert!(
        split_git_conflict("<<<<<<< HEAD\nours\n<<<<<<< HEAD\n=======\ntheirs\n>>>>>>> branch\n")
            .is_none(),
    );
    assert!(
        split_git_conflict("||||||| parent\nbase\n=======\ntheirs\n>>>>>>> branch\n").is_none(),
    );
}
