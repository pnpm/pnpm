use super::declaration_url;
use crate::manifest::Source;

#[test]
fn git_declarations_put_revisions_before_repository_queries() {
    let declaration: Source =
        toml::from_str("git = 'https://example.test/repo.git?key=value'\nrev = 'release#fork'")
            .unwrap();
    let url = declaration_url(&declaration).unwrap().unwrap();
    let pnpm_python_resolver::Source::Git(git) = pnpm_python_resolver::Source::parse(&url).unwrap()
    else {
        panic!("git source expected")
    };
    assert_eq!(git.url, "https://example.test/repo.git?key=value");
    assert_eq!(git.requested_revision, "release#fork");
}
