use super::{NodeBridgeReporter, begin_stats, take_stats};
use pacquet_reporter::{IgnoredScriptsLog, LogEvent, LogLevel, Reporter};

#[test]
fn ignored_scripts_are_returned_as_dependencies_requiring_build() {
    begin_stats();
    NodeBridgeReporter::emit(&LogEvent::IgnoredScripts(IgnoredScriptsLog {
        level: LogLevel::Debug,
        package_names: vec!["foo@1.0.0".to_owned(), "bar@2.0.0".to_owned()],
        strict_dep_builds: false,
    }));
    NodeBridgeReporter::emit(&LogEvent::IgnoredScripts(IgnoredScriptsLog {
        level: LogLevel::Debug,
        package_names: vec!["foo@1.0.0".to_owned()],
        strict_dep_builds: false,
    }));

    assert_eq!(take_stats().deps_requiring_build, ["foo@1.0.0", "bar@2.0.0"]);
}
