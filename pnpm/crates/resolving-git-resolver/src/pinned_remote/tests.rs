use std::{net::SocketAddr, sync::Arc};

use pnpm_network::{GuardedDnsResolver, is_public_address};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};

use super::pin_with;

struct FixedResolver(Vec<SocketAddr>);

impl Resolve for FixedResolver {
    fn resolve(&self, _name: Name) -> Resolving {
        let addresses = self.0.clone();
        Box::pin(async move { Ok(Box::new(addresses.into_iter()) as Addrs) })
    }
}

fn public_only(addresses: &[&str]) -> GuardedDnsResolver {
    GuardedDnsResolver::new(
        Arc::new(FixedResolver(
            addresses
                .iter()
                .map(|address| address.parse().unwrap())
                .collect(),
        )),
        Arc::new(|_, address| is_public_address(address)),
    )
}

/// <https://github.com/pnpm/pnpm/issues/12705>
#[tokio::test]
async fn pins_an_https_remote_to_the_checked_addresses() {
    let resolver = public_only(&["8.8.8.8:0", "[2606:4700:4700::1111]:0"]);
    assert_eq!(
        pin_with("https://git.example/org/repo.git", &resolver).await.unwrap(),
        [
            "http.curloptResolve=git.example:443:8.8.8.8,[2606:4700:4700::1111]",
            "http.followRedirects=false",
        ],
    );
    assert_eq!(
        pin_with("git+http://git.example:8080/org/repo.git", &resolver).await.unwrap()[0],
        "http.curloptResolve=git.example:8080:8.8.8.8,[2606:4700:4700::1111]",
    );
}

/// <https://github.com/pnpm/pnpm/issues/12705>
#[tokio::test]
async fn refuses_a_remote_whose_name_resolves_to_a_refused_address() {
    let resolver = public_only(&["169.254.169.254:0"]);
    let error = pin_with("https://git.example/org/repo.git", &resolver).await.unwrap_err();
    assert!(error.message.contains("git.example resolves to 169.254.169.254"), "{error}");
}

#[tokio::test]
async fn leaves_remotes_it_cannot_pin_alone() {
    let resolver = public_only(&["169.254.169.254:0"]);
    for repo in [
        "ssh://git@git.example/org/repo.git",
        "git://git.example/org/repo.git",
        "https://10.0.0.1/org/repo.git",
        "git@git.example:org/repo.git",
    ] {
        assert!(pin_with(repo, &resolver).await.unwrap().is_empty(), "pinned {repo}");
    }
}
