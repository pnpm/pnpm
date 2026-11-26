use super::node_runtime_specifier;
use pretty_assertions::assert_eq;

#[test]
fn node_requests_read_the_release_index() {
    for (request, specifier) in [
        ("node", "latest"),
        ("node@22", "22"),
        ("node@lts", "lts"),
        ("node@runtime:rc/24", "rc/24"),
    ] {
        assert_eq!(node_runtime_specifier(request).as_deref(), Some(specifier), "{request}");
    }
}

#[test]
fn other_requests_read_the_registry() {
    for request in ["node@npm:node", "nodemon", "@types/node", "deno", "bun@1"] {
        assert_eq!(node_runtime_specifier(request), None, "{request}");
    }
}
