use super::{ComVer, LockfileVersion, LockfileVersionError};
use pipe_trait::Pipe;
use pretty_assertions::assert_eq;

#[test]
fn compatible() {
    macro_rules! case {
        ($major:expr, $input:expr => $output:expr) => {{
            const MAJOR: u16 = $major;
            let input = $input;
            eprintln!("CASE: LockfileVersion::<{MAJOR}>::try_from({input:?})");
            let received: LockfileVersion<MAJOR> = serde_saphyr::from_str(input).unwrap();
            let expected = LockfileVersion::<MAJOR>($output);
            assert_eq!(&received, &expected);
        }};
    }

    case!(9, "9.0" => ComVer { major: 9, minor: 0 });
    case!(9, "9.1" => ComVer { major: 9, minor: 1 });
    case!(6, "6.0" => ComVer { major: 6, minor: 0 });
}

#[test]
fn incompatible() {
    let error = "6.0".parse::<ComVer>().unwrap().pipe(LockfileVersion::<9>::try_from).unwrap_err();
    dbg!(&error);
    assert_eq!(error.to_string(), "The lockfileVersion of 6.0 is incompatible with 9.x");
    assert!(matches!(
        error,
        LockfileVersionError::IncompatibleMajor(ComVer { major: 6, minor: 0 }),
    ));
}
