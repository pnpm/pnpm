use super::{effective_user_id, real_user_id};

/// Pins that the two identity calls agree with the `id` the shell reports,
/// which is the only independent source of the answer available in a test.
#[test]
fn identities_match_the_shell() {
    let reported = |flag: &str| {
        let output = std::process::Command::new("id")
            .arg(flag)
            .output()
            .expect("run id");
        assert!(output.status.success(), "id {flag} failed: {output:?}");
        String::from_utf8(output.stdout)
            .expect("id prints UTF-8")
            .trim()
            .parse::<u32>()
            .expect("id prints a number")
    };
    assert_eq!(effective_user_id(), reported("-u"));
    assert_eq!(real_user_id(), reported("-ur"));
}

/// Pins that the calls are pure, which is what lets callers treat the
/// identity as fixed for the length of the process.
#[test]
fn identities_do_not_change() {
    assert_eq!(effective_user_id(), effective_user_id());
    assert_eq!(real_user_id(), real_user_id());
}
