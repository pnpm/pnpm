use super::SearchArgs;

#[test]
fn search_url_preserves_path_prefix() {
    let args = SearchArgs {
        json: false,
        search_limit: Some(10),
        registry: None,
        query: vec!["express".to_string()],
    };
    let url = args.search_url("https://registry.example/npm/", "express").unwrap();
    assert_eq!(url.as_str(), "https://registry.example/npm/-/v1/search?text=express&size=10");
    let root_url = args.search_url("https://registry.example/", "express").unwrap();
    assert_eq!(root_url.as_str(), "https://registry.example/-/v1/search?text=express&size=10");
}
