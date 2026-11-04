use crate::{
    PackedPkg, PackedVariant, PublishNetwork, PublishPackedPkgOptions, PublishRegistryOptions,
    ReleaseNotesTarget, StagedReference, VariantArchive, batch_publish_packed_pkgs,
    oidc::OidcHttpOptions,
};
use pnpm_network::{AuthHeaders, ThrottledClient};
use pnpm_reporter::SilentReporter;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashSet},
    sync::{Arc, Mutex},
    time::Duration,
};

fn package<'a>(
    manifest: &'a Value,
    tarball_data: &'a [u8],
    variants: &'a [PackedVariant],
) -> PackedPkg<'a> {
    PackedPkg {
        published_manifest: manifest,
        tarball_data,
        tarball_path: "package.tgz",
        contents: &[],
        unpacked_size: 0,
        notes: None,
        variants,
        any_staged: None,
    }
}

fn options(registry: &str) -> PublishPackedPkgOptions {
    PublishPackedPkgOptions {
        dry_run: false,
        stage: false,
        wait_timeout: Duration::ZERO,
        registry: PublishRegistryOptions {
            default: registry.to_owned(),
            scoped: BTreeMap::new(),
            access: None,
            tag: "latest".to_owned(),
            otp: None,
            provenance: None,
            http: OidcHttpOptions::default(),
        },
        release_notes: ReleaseNotesTarget::default(),
        pnpr_registries: HashSet::from([registry.to_owned()]),
    }
}

/// The batch body a publish of `packages` to a mocked pnpr sends, with the
/// integrities of the archives it staged first.
async fn published_batch(
    packages: &[PackedPkg<'_>],
    staged_before: &[&str],
) -> (Value, Vec<String>) {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let uploads = Arc::new(Mutex::new(Vec::new()));
    let blob = mockito::Matcher::Regex("^/-/pnpr/v0/artifacts/blob".to_owned());
    // Each archive staged before is held, at seven bytes; the registry holds
    // no other.
    for integrity in staged_before {
        server
            .mock("HEAD", blob.clone())
            .match_query(mockito::Matcher::UrlEncoded("integrity".into(), (*integrity).into()))
            .with_status(200)
            .with_header("content-length", "7")
            .create_async()
            .await;
    }
    if staged_before.is_empty() {
        server
            .mock("HEAD", blob.clone())
            .with_status(404)
            .create_async()
            .await;
    }
    let recorded = Arc::clone(&uploads);
    server
        .mock("PUT", blob)
        .with_status(201)
        .with_body_from_request(move |request| {
            let url = request.path_and_query().to_owned();
            recorded.lock().unwrap().push(url);
            Vec::new()
        })
        .create_async()
        .await;
    let body = Arc::new(Mutex::new(Value::Null));
    let sent = Arc::clone(&body);
    server
        .mock("PUT", "/-/pnpr/v0/publish")
        .with_status(200)
        .with_body_from_request(move |request| {
            *sent.lock().unwrap() = serde_json::from_slice(request.body().unwrap()).unwrap();
            b"{}".to_vec()
        })
        .create_async()
        .await;
    let client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let network = PublishNetwork { client: &client, auth_headers: &auth_headers };
    batch_publish_packed_pkgs::<SilentReporter, _, crate::batch_publish::BatchPublishError>(
        packages,
        &options(&registry),
        &network,
        |_| Ok(()),
    )
    .await
    .expect("the batch publishes");
    let uploaded = uploads
        .lock()
        .unwrap()
        .iter()
        .map(|url| {
            let encoded = url.split_once("integrity=").unwrap().1;
            percent_decode(encoded)
        })
        .collect();
    let body = body.lock().unwrap().clone();
    (body, uploaded)
}

fn sri_sha512(bytes: &[u8]) -> String {
    ssri::IntegrityOpts::new()
        .algorithm(ssri::Algorithm::Sha512)
        .chain(bytes)
        .result()
        .to_string()
}

fn percent_decode(text: &str) -> String {
    text.replace("%2B", "+")
        .replace("%2F", "/")
        .replace("%3D", "=")
}

#[tokio::test]
async fn a_batch_stages_every_archive_of_a_version_with_variants_and_references_them() {
    let cli = json!({ "name": "example-cli", "version": "1.0.0" });
    let lib = json!({ "name": "example-lib", "version": "1.0.0" });
    let variants = [PackedVariant {
        tag: "linux-x64".to_owned(),
        archive: VariantArchive::Packed(b"linux".to_vec()),
    }];
    let packages = [package(&cli, b"any", &variants), package(&lib, b"lib", &[])];
    let (body, mut uploaded) = published_batch(&packages, &[]).await;

    let mut expected = vec![sri_sha512(b"any"), sri_sha512(b"linux"), sri_sha512(b"lib")];
    uploaded.sort();
    expected.sort();
    assert_eq!(uploaded, expected);
    let cli_attachments = &body["packages"][0]["_attachments"];
    assert_eq!(cli_attachments["example-cli-1.0.0.tgz"]["integrity"], sri_sha512(b"any"));
    assert_eq!(
        cli_attachments["example-cli-1.0.0~linux-x64.tgz"],
        json!({
            "content_type": "application/octet-stream",
            "integrity": sri_sha512(b"linux"),
            "length": 5,
        }),
    );
    let lib_attachment = &body["packages"][1]["_attachments"]["example-lib-1.0.0.tgz"];
    assert_eq!(lib_attachment["integrity"], sri_sha512(b"lib"));
    assert!(lib_attachment.get("data").is_none(), "{lib_attachment}");
}

#[tokio::test]
async fn a_batch_references_variants_another_job_staged_by_their_size() {
    let cli = json!({ "name": "example-cli", "version": "1.0.0" });
    let staged = "sha512-c3RhZ2Vk";
    let variants = [PackedVariant {
        tag: "linux-x64".to_owned(),
        archive: VariantArchive::Staged(StagedReference {
            integrity: staged.to_owned(),
            label: "linux-x64".to_owned(),
        }),
    }];
    let packages = [package(&cli, b"", &variants)];
    let (body, uploaded) = published_batch(&packages, &[staged]).await;

    assert!(uploaded.is_empty(), "{uploaded:?}");
    let document = &body["packages"][0];
    assert_eq!(
        document["_attachments"],
        json!({
            "example-cli-1.0.0~linux-x64.tgz": {
                "content_type": "application/octet-stream",
                "integrity": staged,
                "length": 7,
            },
        }),
    );
    assert!(document["versions"]["1.0.0"].get("dist").is_none(), "{document}");
}
