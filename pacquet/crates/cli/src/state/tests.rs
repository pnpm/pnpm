use super::call_load_lockfile;
use pretty_assertions::assert_eq;

#[test]
fn test_call_load_lockfile() {
    macro_rules! case {
        ($should_load:expr, $load_lockfile:expr => $output:expr) => {{
            let should_load = $should_load;
            let load_lockfile = $load_lockfile;
            let output: Result<Option<&str>, &str> = $output;
            eprintln!(
                "CASE: {should_load:?}, {load_lockfile} => {output:?}",
                load_lockfile = stringify!($load_lockfile),
            );
            assert_eq!(call_load_lockfile(should_load, load_lockfile), output);
        }};
    }

    case!(false, || unreachable!() => Ok(None));
    case!(true, || Err("error") => Err("error"));
    case!(true, || Ok(None) => Ok(None));
    case!(true, || Ok(Some("value")) => Ok(Some("value")));
}
