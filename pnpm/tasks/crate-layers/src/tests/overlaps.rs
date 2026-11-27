use crate::{
    overlaps::{
        self,
        Group,
    },
    program::{
        Field,
        Function,
        Place,
        Program,
    },
};
use pretty_assertions::assert_eq;
use std::collections::BTreeSet;

/// A function of the crate `one` on line `line` of its module's file.
pub(super) fn function(
    module: &str,
    line: usize,
    accessed: &[usize],
    called: &[usize],
) -> Function {
    Function {
        name: format!("{module}_{line}"),
        place: Place { crate_name: "one".to_owned(), module: module.to_owned() },
        file: format!("crates/one/src/{module}.rs"),
        line,
        accessed: accessed.iter().copied().collect(),
        called: called.iter().copied().collect(),
    }
}

/// A program whose fields `0` to `5` belong to `one::State` and `6` to `9`
/// to `one::Other`.
pub(super) fn program(functions: Vec<Function>) -> Program {
    let field =
        |index: usize| Field { owner: usize::from(index > 5), name: format!("field_{index}") };
    Program {
        functions,
        fields: (0..10).map(field).collect(),
        types: vec!["one::State".to_owned(), "one::Other".to_owned()],
    }
}

fn members(groups: &[Group]) -> Vec<Vec<usize>> {
    groups
        .iter()
        .map(|group| group.functions.clone())
        .collect()
}

#[test]
fn pairs_functions_in_different_modules_that_access_the_same_fields() {
    let program = program(vec![
        function("left", 1, &[0, 1, 2, 3], &[]),
        function("right", 1, &[0, 1, 2, 3, 6], &[]),
        function("left", 9, &[0, 1, 2, 3], &[]),
    ]);
    let groups = overlaps::find(&program, 4);
    dbg!(&groups);
    assert_eq!(
        groups,
        vec![Group { functions: vec![0, 2, 1], shared: 4, fields: BTreeSet::from([0, 1, 2, 3]) }],
    );
}

#[test]
fn leaves_out_a_pair_that_shares_too_few_fields() {
    let fewer_than_min_shared =
        program(vec![function("left", 1, &[0, 1, 2], &[]), function("right", 1, &[0, 1, 2], &[])]);
    let less_than_half_of_either = program(vec![
        function("left", 1, &[0, 1, 2, 3], &[]),
        function("right", 1, &[0, 1, 2, 3, 4, 5, 6, 7, 8], &[]),
    ]);
    assert_eq!(members(&overlaps::find(&fewer_than_min_shared, 4)), Vec::<Vec<usize>>::new());
    assert_eq!(members(&overlaps::find(&less_than_half_of_either, 4)), Vec::<Vec<usize>>::new());
    assert_eq!(members(&overlaps::find(&fewer_than_min_shared, 3)), vec![vec![0, 1]]);
}

#[test]
fn leaves_out_a_pair_when_one_function_reaches_the_other_through_calls() {
    let through_middle = program(vec![
        function("left", 1, &[0, 1, 2, 3], &[2]),
        function("right", 1, &[0, 1, 2, 3], &[]),
        function("middle", 1, &[], &[1]),
    ]);
    let unrelated = program(vec![
        function("left", 1, &[0, 1, 2, 3], &[2]),
        function("right", 1, &[0, 1, 2, 3], &[]),
        function("middle", 1, &[], &[]),
    ]);
    assert_eq!(members(&overlaps::find(&through_middle, 4)), Vec::<Vec<usize>>::new());
    assert_eq!(members(&overlaps::find(&unrelated, 4)), vec![vec![0, 1]]);
}

#[test]
fn leaves_out_a_pair_in_one_module() {
    let program = program(vec![
        function("left", 1, &[0, 1, 2, 3], &[]),
        function("left", 9, &[0, 1, 2, 3], &[]),
    ]);
    assert_eq!(members(&overlaps::find(&program, 4)), Vec::<Vec<usize>>::new());
}

#[test]
fn joins_overlaps_that_have_a_function_in_common_and_puts_the_largest_first() {
    let program = program(vec![
        function("left", 1, &[0, 1, 2, 3], &[]),
        function("middle", 1, &[0, 1, 2, 3, 4, 5], &[]),
        function("right", 1, &[2, 3, 4, 5], &[]),
        function("left", 2, &[6, 7, 8, 9, 0], &[]),
        function("right", 2, &[6, 7, 8, 9, 0], &[]),
    ]);
    let groups = overlaps::find(&program, 4);
    dbg!(&groups);
    assert_eq!(members(&groups), vec![vec![3, 4], vec![0, 1, 2]]);
    assert_eq!(
        overlaps::render(&program, &groups),
        "\
2 functions share 5 fields: one::Other (4), one::State (1)
  crates/one/src/left.rs:2 left_2
  crates/one/src/right.rs:2 right_2
3 functions, two of which share 4 fields: one::State (6)
  crates/one/src/left.rs:1 left_1
  crates/one/src/middle.rs:1 middle_1
  crates/one/src/right.rs:1 right_1
",
    );
}
