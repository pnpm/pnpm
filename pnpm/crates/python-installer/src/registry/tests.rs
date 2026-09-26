use super::CachedIndex;
use pnpm_network::SecureAuthResponse;

#[test]
fn truncated_404_responses_are_missing_pages_but_truncated_successes_are_rejected() {
    let name = "alpha".parse().unwrap();
    for status in [404, 200] {
        let page = CachedIndex::from_response(
            &name,
            &SecureAuthResponse {
                status: status.try_into().unwrap(),
                body: Vec::new(),
                body_truncated: true,
                url: "https://example.test/simple/alpha/".to_string(),
            },
        );
        if status == 404 {
            assert!(page.unwrap().missing);
        } else {
            assert!(
                page.err()
                    .unwrap()
                    .to_string()
                    .contains("exceeds"),
            );
        }
    }
}
