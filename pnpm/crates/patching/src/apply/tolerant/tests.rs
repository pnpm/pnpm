use super::apply;
use diffy::Patch;
use pretty_assertions::assert_eq;

fn applied(original: &str, patch: &str) -> String {
    let patch = Patch::from_str(patch).expect("parse patch");
    apply(original, &patch).expect("apply patch")
}

/// A patch whose final hunk line is context and whose file has no final
/// newline. `git diff` marks the context line rather than an
/// insertion/deletion, which a byte-exact matcher reads as a missing
/// newline and rejects.
#[test]
fn applies_when_the_last_hunk_line_is_context_without_a_final_newline() {
    let original = "one\ntwo\nthree\nfour";
    let patch = "\
--- a/file.txt
+++ b/file.txt
@@ -1,4 +1,4 @@
 one
-two
+TWO
 three
 four
\\ No newline at end of file
";
    assert_eq!(applied(original, patch), "one\nTWO\nthree\nfour");
}

/// The same shape with a final newline still round-trips.
#[test]
fn keeps_a_final_newline_when_the_file_has_one() {
    let original = "one\ntwo\nthree\nfour\n";
    let patch = "\
--- a/file.txt
+++ b/file.txt
@@ -1,4 +1,4 @@
 one
-two
+TWO
 three
 four
";
    assert_eq!(applied(original, patch), "one\nTWO\nthree\nfour\n");
}

/// An LF patch against a CRLF file. The replaced line takes the patch's
/// LF ending; every untouched line keeps its `\r`.
#[test]
fn applies_an_lf_patch_to_a_crlf_file() {
    let original = "one\r\ntwo\r\nthree\r\n";
    let patch = "\
--- a/file.txt
+++ b/file.txt
@@ -1,3 +1,3 @@
 one
-two
+TWO
 three
";
    assert_eq!(applied(original, patch), "one\r\nTWO\nthree\r\n");
}

/// A hunk whose recorded line numbers drifted still applies, up to
/// twenty lines either side.
#[test]
fn finds_a_hunk_that_moved_within_the_fuzzing_window() {
    let original = "pad\npad\npad\none\ntwo\nthree\n";
    let patch = "\
--- a/file.txt
+++ b/file.txt
@@ -1,3 +1,3 @@
 one
-two
+TWO
 three
";
    assert_eq!(applied(original, patch), "pad\npad\npad\none\nTWO\nthree\n");
}

/// Dropping the final newline is expressed as a replacement pair whose
/// insertion carries the annotation.
#[test]
fn removes_a_final_newline_when_the_insertion_is_annotated() {
    let original = "one\ntwo\n";
    let patch = "\
--- a/file.txt
+++ b/file.txt
@@ -1,2 +1,2 @@
 one
-two
+TWO
\\ No newline at end of file
";
    assert_eq!(applied(original, patch), "one\nTWO");
}

/// Adding a final newline: only the deletion carries the annotation.
#[test]
fn adds_a_final_newline_when_only_the_deletion_is_annotated() {
    let original = "one\ntwo";
    let patch = "\
--- a/file.txt
+++ b/file.txt
@@ -1,2 +1,2 @@
 one
-two
\\ No newline at end of file
+TWO
";
    assert_eq!(applied(original, patch), "one\nTWO\n");
}

/// Context that matches nothing is still a failure — the tolerances
/// widen what counts as a match, they don't make the applier accept a
/// patch written against different content.
#[test]
fn rejects_a_hunk_whose_context_does_not_match() {
    let patch = Patch::from_str(
        "\
--- a/file.txt
+++ b/file.txt
@@ -1,2 +1,2 @@
 nowhere
-two
+TWO
",
    )
    .expect("parse patch");
    let error = apply("one\ntwo\n", &patch).expect_err("must not apply");
    assert_eq!(error, "error applying hunk #1");
}

/// Several hunks are located against the unpatched file, so an earlier
/// hunk's edits don't shift a later one out from under its own match.
#[test]
fn applies_several_hunks_against_the_same_baseline() {
    let original = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\n";
    let patch = "\
--- a/file.txt
+++ b/file.txt
@@ -1,3 +1,4 @@
 a
+A
 b
 c
@@ -10,3 +11,3 @@
 j
-k
+K
 l
";
    assert_eq!(applied(original, patch), "a\nA\nb\nc\nd\ne\nf\ng\nh\ni\nj\nK\nl\n");
}
