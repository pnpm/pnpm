use miette::{Result, WrapErr};
use pnpm_pnpr_client::PnprClient;
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
};

/// Whether `pnpr_server` advertises resolution for `ecosystem`. Asked once
/// per server and ecosystem for the life of the process, so a server that
/// gains an ecosystem while an install runs is not noticed until the next
/// one.
pub(crate) async fn server_resolves(
    client: &PnprClient,
    pnpr_server: &str,
    ecosystem: &str,
) -> Result<bool> {
    let answer = Arc::clone(
        ANSWERS
            .lock()
            .expect("pnpr ecosystem memo is poisoned")
            .entry((pnpr_server.to_string(), ecosystem.to_string()))
            .or_default(),
    );
    answer
        .get_or_init(|| async {
            client.supports_ecosystem(ecosystem).await.map_err(|err| err.to_string())
        })
        .await
        .as_ref()
        .copied()
        .map_err(|err| miette::miette!("{err}"))
        .wrap_err_with(|| format!("ask the pnpr server whether it resolves {ecosystem}"))
}

/// A server's answer, or the failure to get one. The failure is kept too:
/// a server that cannot be reached cannot be reached for the next root
/// either, and retrying per root would serialize one timeout per root
/// behind the shared cell.
type Answer = Arc<tokio::sync::OnceCell<Result<bool, String>>>;

static ANSWERS: LazyLock<Mutex<HashMap<(String, String), Answer>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
