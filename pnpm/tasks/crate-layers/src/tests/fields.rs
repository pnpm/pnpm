use super::overlaps::{
    function,
    program,
};
use crate::fields;
use pretty_assertions::assert_eq;

#[test]
fn lists_the_types_whose_fields_two_modules_both_access() {
    let program = program(vec![
        function("left", 1, &[0, 1], &[]),
        function("left", 12, &[2, 6, 7], &[]),
        function("right", 3, &[1, 2, 3, 6], &[]),
        function("other", 1, &[5], &[]),
    ]);
    let uses = fields::find(&program);
    dbg!(&uses);
    assert_eq!(
        fields::render_summary(&program, &uses, 2),
        "type | fields two modules both access | modules\none::State | 2 | 3\n",
    );
    assert_eq!(
        fields::render_summary(&program, &uses, 1),
        "type | fields two modules both access | modules\none::State | 2 | 3\none::Other | 1 | 2\n",
    );
}

#[test]
fn lists_the_functions_that_access_one_type_by_module() {
    let program = program(vec![
        function("left", 1, &[0, 1], &[]),
        function("left", 12, &[2, 6, 7], &[]),
        function("left", 5, &[1, 2], &[]),
        function("right", 3, &[1, 2, 3, 4, 6], &[]),
    ]);
    let uses = fields::find(&program);
    dbg!(&uses);
    assert_eq!(
        fields::render_type(&program, &uses, "one::State").unwrap(),
        "\
one::State: 5 fields accessed in 2 modules
one::right: 4 fields in 1 functions
  crates/one/src/right.rs:3 right_3 (4)
one::left: 3 fields in 3 functions
  crates/one/src/left.rs:1 left_1 (2)
  crates/one/src/left.rs:5 left_5 (2)
  crates/one/src/left.rs:12 left_12 (1)
",
    );
    assert_eq!(fields::render_type(&program, &uses, "one::Missing"), None);
}
