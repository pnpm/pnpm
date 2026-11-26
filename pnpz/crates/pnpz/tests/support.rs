//! A catalog and a signed binary cache served from one mock HTTP server.

use base64::{
    Engine,
    engine::general_purpose::STANDARD as BASE64,
};
use ed25519_dalek::{
    Signer,
    SigningKey,
};
use mockito::{
    Mock,
    Server,
    ServerGuard,
};
use pnpm_network::ThrottledClient;
use pnpz::{
    binary_cache::BinaryCache,
    catalog::Catalog,
    store::Store,
};
use pnpz_nix_store::{
    NarHash,
    StorePath,
    TrustedKey,
    base32,
    nar,
};
use sha2::{
    Digest,
    Sha256,
};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    sync::Arc,
};
use tempfile::TempDir;

const KEY_NAME: &str = "test-cache-1";
pub const SYSTEM: &str = "x86_64-linux";

pub struct Remote {
    pub server: ServerGuard,
    signing_key: SigningKey,
    temp: TempDir,
}

/// How a test wants an archive served.
#[derive(Clone, Copy)]
pub enum Served {
    Zstd,
    Xz,
    Uncompressed,
    /// The archive of a different tree than the narinfo signs.
    Tampered,
    /// Signed by a key the client does not trust.
    UntrustedSignature,
}

impl Remote {
    pub async fn start() -> Self {
        Self {
            server: Server::new_async().await,
            signing_key: SigningKey::from_bytes(&[7; 32]),
            temp: TempDir::new().unwrap(),
        }
    }

    pub fn public_key(&self) -> String {
        format!("{KEY_NAME}:{}", BASE64.encode(self.signing_key.verifying_key().to_bytes()))
    }

    pub fn store(&self) -> Store {
        Store::at(self.temp.path().join("nix")).unwrap()
    }

    pub fn clients(&self) -> (Catalog, BinaryCache) {
        let client = Arc::new(ThrottledClient::default());
        let keys = vec![TrustedKey::parse(&self.public_key()).unwrap()];
        let catalog = Catalog::new(Arc::clone(&client), &self.server.url());
        (catalog, BinaryCache::new(client, &self.server.url(), keys))
    }

    /// Serves a store path holding the executable `bin/<name>`, which prints
    /// `output`, and returns the path and the mock of its archive.
    pub async fn add_path(
        &mut self,
        name: &str,
        output: &str,
        references: &[&StorePath],
        served: Served,
    ) -> (StorePath, Mock) {
        let store_path = store_path(name);
        let tree = self.temp
            .path()
            .join(format!("tree-{name}"));
        write_tool(&tree, name, output);
        let archive = packed(&tree);
        let body = match served {
            Served::Tampered => {
                write_tool(
                    &self.temp
                        .path()
                        .join(format!("other-{name}")),
                    name,
                    "tampered",
                );
                packed(
                    &self.temp
                        .path()
                        .join(format!("other-{name}")),
                )
            }
            _ => archive.clone(),
        };
        let (extension, compression, body) = compressed(served, body);
        let narinfo = self.narinfo(&store_path, &archive, references, served);
        let narinfo = narinfo.replace("{URL}", &format!("nar/{name}.nar{extension}"));
        let narinfo = narinfo.replace("{COMPRESSION}", compression);
        self.server
            .mock("GET", format!("/{}.narinfo", store_path.hash_part()).as_str())
            .with_body(narinfo)
            .create_async()
            .await;
        let archive_mock = self.server
            .mock("GET", format!("/nar/{name}.nar{extension}").as_str())
            .with_body(body)
            .create_async()
            .await;
        (store_path, archive_mock)
    }

    fn narinfo(
        &self,
        path: &StorePath,
        archive: &[u8],
        references: &[&StorePath],
        served: Served,
    ) -> String {
        let nar_hash = NarHash::from_digest(Sha256::digest(archive).into()).to_nix_base32();
        let absolute: Vec<String> = references
            .iter()
            .map(ToString::to_string)
            .collect();
        let fingerprint = format!("1;{path};{nar_hash};{};{}", archive.len(), absolute.join(","));
        let key_name = match served {
            Served::UntrustedSignature => "someone-else-1",
            _ => KEY_NAME,
        };
        let signature = BASE64.encode(self.signing_key.sign(fingerprint.as_bytes()).to_bytes());
        let base_names: Vec<&str> = references
            .iter()
            .map(|path| path.base_name())
            .collect();
        format!(
            "StorePath: {path}\nURL: {{URL}}\nCompression: {{COMPRESSION}}\nNarHash: {nar_hash}\nNarSize: {}\nReferences: {}\nSig: {key_name}:{signature}\n",
            archive.len(),
            base_names.join(" "),
        )
    }

    /// Serves the catalog entry of `name`, resolved to `version` and `path`.
    pub async fn add_package(&mut self, name: &str, version: &str, path: &StorePath) -> Mock {
        let body = format!(
            r#"{{"name":"{name}","version":"{version}","systems":{{"{SYSTEM}":{{
                "flake_installable":{{"ref":{{"rev":"0123abc"}},"attr_path":"{name}"}},
                "outputs":[{{"name":"out","path":"{path}","default":true}},
                           {{"name":"debug","path":"{path}-debug"}}]}}}}}}"#,
        );
        self.server
            .mock("GET", "/v2/resolve")
            .match_query(mockito::Matcher::UrlEncoded("name".into(), name.into()))
            .with_body(body)
            .create_async()
            .await
    }
}

/// A valid store path whose hash is derived from `name`.
pub fn store_path(name: &str) -> StorePath {
    let hash = base32::encode(&Sha256::digest(name)[..20]);
    StorePath::from_base_name(&format!("{hash}-{name}")).unwrap()
}

fn write_tool(tree: &Path, name: &str, output: &str) {
    fs::create_dir_all(tree.join("bin")).unwrap();
    let tool = tree.join("bin").join(name);
    fs::write(&tool, format!("#!/bin/sh\necho {output}\n")).unwrap();
    fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();
}

fn packed(tree: &Path) -> Vec<u8> {
    let mut archive = Vec::new();
    nar::pack(tree, &mut archive).unwrap();
    archive
}

fn compressed(served: Served, archive: Vec<u8>) -> (&'static str, &'static str, Vec<u8>) {
    match served {
        Served::Uncompressed => ("", "none", archive),
        Served::Xz => {
            let mut writer =
                lzma_rust2::XzWriter::new(Vec::new(), lzma_rust2::XzOptions::default()).unwrap();
            std::io::Write::write_all(&mut writer, &archive).unwrap();
            (".xz", "xz", writer.finish().unwrap())
        }
        _ => (".zst", "zstd", zstd::encode_all(archive.as_slice(), 3).unwrap()),
    }
}
