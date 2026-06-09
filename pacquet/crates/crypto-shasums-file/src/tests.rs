use pretty_assertions::assert_eq;

use super::{
    FetchVerifiedNodeShasumsError, PickFileChecksumError, ShasumsFileItem,
    fetch_verified_node_shasums, is_signed_by_trusted_node_release_key, parse_shasums_file,
    pick_file_checksum_from_shasums_file,
};

/// Two valid rows are parsed into SRI-encoded integrities.
///
/// Mirrors upstream's first
/// [`pickFileChecksumFromShasumsFile` test](https://github.com/pnpm/pnpm/blob/1627943d2a/crypto/shasums-file/test/index.ts#L5-L8)
/// — same input body, same expected integrity for the first row.
#[test]
fn parses_rows_into_sri_encoded_integrities() {
    let body = "\
ed52239294ad517fbe91a268146d5d2aa8a17d2d62d64873e43219078ba71c4e  foo.tar.gz
be127be1d98cad94c56f46245d0f2de89934d300028694456861a6d5ac558bf3  foo.msi
";
    let items = parse_shasums_file(body);
    assert_eq!(
        items,
        vec![
            ShasumsFileItem {
                integrity: "sha256-7VIjkpStUX++kaJoFG1dKqihfS1i1khz5DIZB4unHE4=".to_string(),
                file_name: "foo.tar.gz".to_string(),
            },
            ShasumsFileItem {
                integrity: "sha256-vhJ74dmMrZTFb0YkXQ8t6Jk00wAChpRFaGGm1axVi/M=".to_string(),
                file_name: "foo.msi".to_string(),
            },
        ],
    );
}

/// Empty lines anywhere in the body are dropped.
#[test]
fn skips_empty_lines() {
    let body = "\n\nabc def\n";
    // The hash isn't valid hex, so this is just exercising the line
    // split. We rely on the upstream contract that the body has a
    // proper hash; the picker is the validator.
    let items = parse_shasums_file(body);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].file_name, "def");
}

/// Picking the integrity for an existing filename succeeds.
///
/// Mirrors the
/// [`picks the right checksum for a file`](https://github.com/pnpm/pnpm/blob/1627943d2a/crypto/shasums-file/test/index.ts#L5-L8)
/// upstream test verbatim.
#[test]
fn picks_the_right_checksum_for_a_file() {
    let body = "\
ed52239294ad517fbe91a268146d5d2aa8a17d2d62d64873e43219078ba71c4e  foo.tar.gz
be127be1d98cad94c56f46245d0f2de89934d300028694456861a6d5ac558bf3  foo.msi";
    let integrity = pick_file_checksum_from_shasums_file(body, "foo.tar.gz").unwrap();
    assert_eq!(integrity, "sha256-7VIjkpStUX++kaJoFG1dKqihfS1i1khz5DIZB4unHE4=");
}

/// Picking a filename that isn't in the body raises `NODE_INTEGRITY_HASH_NOT_FOUND`.
///
/// Mirrors the
/// [`throws an error if no integrity found`](https://github.com/pnpm/pnpm/blob/1627943d2a/crypto/shasums-file/test/index.ts#L9-L12)
/// upstream test.
#[test]
fn missing_file_name_raises_not_found() {
    let body = "\
ed52239294ad517fbe91a268146d5d2aa8a17d2d62d64873e43219078ba71c4e  foo.tar.gz
be127be1d98cad94c56f46245d0f2de89934d300028694456861a6d5ac558bf3  foo.msi";
    let err = pick_file_checksum_from_shasums_file(body, "bar.zip").unwrap_err();
    assert!(matches!(
        err,
        PickFileChecksumError::NotFound { ref file_name } if file_name == "bar.zip",
    ));
}

/// A malformed (too-short) hash in an otherwise well-formed row raises
/// `NODE_MALFORMED_INTEGRITY_HASH` instead of silently truncating the
/// integrity.
///
/// Mirrors the
/// [`throws an error if a malformed integrity is found`](https://github.com/pnpm/pnpm/blob/1627943d2a/crypto/shasums-file/test/index.ts#L13-L16)
/// upstream test.
#[test]
fn malformed_hash_raises_malformed() {
    let body = "\
ed52239294ad517fbe91  foo.tar.gz
be127be1d98cad94c56f46245d0f2de89934d300028694456861a6d5ac558bf3  foo.msi";
    let err = pick_file_checksum_from_shasums_file(body, "foo.tar.gz").unwrap_err();
    match err {
        PickFileChecksumError::Malformed { file_name, sha256 } => {
            assert_eq!(file_name, "foo.tar.gz");
            assert_eq!(sha256, "ed52239294ad517fbe91");
        }
        other @ PickFileChecksumError::NotFound { .. } => {
            panic!("expected Malformed, got {other:?}")
        }
    }
}

#[tokio::test]
async fn fetches_node_shasums_when_signature_verifies() {
    let mut server = mockito::Server::new_async().await;
    let signature = node_22_11_0_signature();
    let _shasums = server
        .mock("GET", "/download/release/v22.11.0/SHASUMS256.txt")
        .with_status(200)
        .with_body(NODE_22_11_0_SHASUMS)
        .create_async()
        .await;
    let _signature = server
        .mock("GET", "/download/release/v22.11.0/SHASUMS256.txt.sig")
        .with_status(200)
        .with_body(signature)
        .create_async()
        .await;
    let client = pacquet_network::ThrottledClient::new_for_installs();
    let body = fetch_verified_node_shasums(
        &client,
        &format!("{}/download/release/v22.11.0/SHASUMS256.txt", server.url()),
    )
    .await
    .expect("signature verifies");

    assert_eq!(body, NODE_22_11_0_SHASUMS);
}

#[test]
fn rejects_tampered_node_shasums() {
    let signature = node_22_11_0_signature();
    let tampered = NODE_22_11_0_SHASUMS.replacen("1bbf7e63", "00000000", 1);

    assert!(
        !is_signed_by_trusted_node_release_key(tampered.as_bytes(), &signature)
            .expect("signature parsed"),
    );
}

#[tokio::test]
async fn missing_node_shasums_signature_fails() {
    let mut server = mockito::Server::new_async().await;
    let _shasums = server
        .mock("GET", "/download/release/v22.11.0/SHASUMS256.txt")
        .with_status(200)
        .with_body(NODE_22_11_0_SHASUMS)
        .create_async()
        .await;
    let _signature = server
        .mock("GET", "/download/release/v22.11.0/SHASUMS256.txt.sig")
        .with_status(404)
        .create_async()
        .await;
    let client = pacquet_network::ThrottledClient::new_for_installs();
    let err = fetch_verified_node_shasums(
        &client,
        &format!("{}/download/release/v22.11.0/SHASUMS256.txt", server.url()),
    )
    .await
    .expect_err("missing signature must fail");

    assert!(matches!(
        err,
        FetchVerifiedNodeShasumsError::StatusNotOk { what: "SHASUMS256.txt.sig", status: 404, .. },
    ));
}

fn node_22_11_0_signature() -> Vec<u8> {
    use base64::{Engine, engine::general_purpose::STANDARD as BASE64_STANDARD};

    BASE64_STANDARD.decode(NODE_22_11_0_SIGNATURE_B64).expect("valid base64")
}

// cspell:disable
const NODE_22_11_0_SHASUMS: &str = "\
1bbf7e632ea55eabf920e8e27bb3e73ca4923eca78a300e5767635e9b2c0c603  node-v22.11.0-aix-ppc64.tar.gz
de6cd4db461b6dc3b3eab31a36b58e30d8af074183bcb13ceca6fd162a579ba6  node-v22.11.0-arm64.msi
2e89afe6f4e3aa6c7e21c560d8a0453d84807e97850bbb819b998531a22bdfde  node-v22.11.0-darwin-arm64.tar.gz
c379a90c6aa605b74042a233ddcda4247b347ba5732007d280e44422cc8f9ecb  node-v22.11.0-darwin-arm64.tar.xz
668d30b9512137b5f5baeef6c1bb4c46efff9a761ba990a034fb6b28b9da2465  node-v22.11.0-darwin-x64.tar.gz
ab28d1784625d151e3f608a9412a009118f376118ed842ae643f8c2efdfb0af6  node-v22.11.0-darwin-x64.tar.xz
0d42dc3b3377f49e495976dc0e4f5c3a7ffb1d714050d2f247afdbbc0898dae5  node-v22.11.0-headers.tar.gz
7eddf759cd3d1a0113c1a0ac7c080e5c0e458bca34a064c62dc8ce613ff5efdd  node-v22.11.0-headers.tar.xz
27453f7a0dd6b9e6738f1f6ea6a09b102ec7aa484de1e39d6a1c3608ad47aa6a  node-v22.11.0-linux-arm64.tar.gz
6031d04b98f59ff0f7cb98566f65b115ecd893d3b7870821171708cdbaf7ae6e  node-v22.11.0-linux-arm64.tar.xz
f85ced095b17e2535859fd2a5641370c3fca12dd72147f93d2696e2909fe1e9d  node-v22.11.0-linux-armv7l.tar.gz
9de0fdcfb1cccbe03f72f939e4e6f03867aef3da8223f90606cd93757704dae0  node-v22.11.0-linux-armv7l.tar.xz
0532965a717d3996302a111703c007dac2763e01795730d488dadbc2fcfac2fa  node-v22.11.0-linux-ppc64le.tar.gz
d1d49d7d611b104b6d616e18ac439479d8296aa20e3741432de0e85f4735a81e  node-v22.11.0-linux-ppc64le.tar.xz
64f691400ffe3a84be930e0cb03607d0b95bef122a679f7893d8e2972e90c521  node-v22.11.0-linux-s390x.tar.gz
f474ed77d6b13d66d07589aee1c2b9175be4c1b165483e608ac1674643064a99  node-v22.11.0-linux-s390x.tar.xz
4f862bab52039835efbe613b532238b6e4dde98d139a34e6923193e073438b13  node-v22.11.0-linux-x64.tar.gz
83bf07dd343002a26211cf1fcd46a9d9534219aad42ee02847816940bf610a72  node-v22.11.0-linux-x64.tar.xz
8d658eda7699d580ccc268ca8a40ced5aeecef5bb4d19c4187e92eebac5d68ec  node-v22.11.0.pkg
24e5130fa7bc1eaab218a0c9cb05e03168fa381bb9e3babddc6a11f655799222  node-v22.11.0.tar.gz
bbf0297761d53aefda9d7855c57c7d2c272b83a7b5bad4fea9cb29006d8e1d35  node-v22.11.0.tar.xz
55b491f3d73fdacf8cf43a2199e824abadda2c43a94780310baa526dc1d679e2  node-v22.11.0-win-arm64.7z
b9ff5a6b6ffb68a0ffec82cc5664ed48247dabbd25ee6d129facd2f65a8ca80d  node-v22.11.0-win-arm64.zip
d2a4fadb1f5e4abc634b6ac16c44cae7c73ffc3dbfe8b92b011d85f2df90f6c1  node-v22.11.0-win-x64.7z
905373a059aecaf7f48c1ce10ffbd5334457ca00f678747f19db5ea7d256c236  node-v22.11.0-win-x64.zip
ca0a274f1edc90005b1dc7ec22ec55dad1acc21320bc0be853065d69db2a5152  node-v22.11.0-win-x86.7z
700e0b1bcaca8b1a04c929ce29b0f07e099b4a34a7facab74fda71764d16f71c  node-v22.11.0-win-x86.zip
9eea480bd30c98ae11a97cb89a9278235cbbbd03c171ee5e5198bd86b7965b4b  node-v22.11.0-x64.msi
ab19f02c4b0d9f578928b67d2a652496aa31729a8cc9771ffc9cc6d3b8afe7e3  node-v22.11.0-x86.msi
b4e5e2821aeb518c0c55f02d4fcd9182c57f97bcce50341998333dba38e34ea4  win-arm64/node.exe
ad65afe5b192644fec9d599c77f0e38a8421d0d7ad2389679882a288c8df444b  win-arm64/node.lib
0861cf0f1ff6135a21eb26279fc6a6f7dc9d9c0ac926a17553f387c32945eea5  win-arm64/node_pdb.7z
f35c2d1a967080b0a1e288b891cb9300a04d0b90042bac8c965c9ebcfc3749bf  win-arm64/node_pdb.zip
7447c4ece014aa41fb2ff866c993c708e5a8213a00913cc2ac5049ea3ffc230d  win-x64/node.exe
3581a06b68c4584d146372113eaa8c4d102127222e5041195ba38f185eef419c  win-x64/node.lib
171d80aeedbe43bd70b3539de6f845a359d8dd97a684df2cbb4f49d8946f4991  win-x64/node_pdb.7z
7c3fa0149b17d9ff4b5af2f3e19e768b6ab684a9dd8dcf35ea204a90d3f56903  win-x64/node_pdb.zip
e54a4559dafd56562a45b50000831d28ee2f7f1ac4ff98b38165871f31f64ab8  win-x86/node.exe
45399070d1d247cf223d12e80d3e638635af24d2f7a4714bc8e38a6a918f162a  win-x86/node.lib
a78040dbb0e7296eebe90c235091ee46a8a01587a226bf4e5a01f5b399e153d7  win-x86/node_pdb.7z
9fb300178536e8243ad55207ee85990731e77299c9e670cec0b54e10dc971713  win-x86/node_pdb.zip
";

const NODE_22_11_0_SIGNATURE_B64: &str = concat!(
    "iQIzBAABCAAdFiEEyC+jrhy+3Gvka5NgxDzsRcF6uTwFAmcg6xUACgkQxDzsRcF6",
    "uTyvgg/+J2t1XkjQGIKUiZVKbdWcct6x4F9RblpmlVV4ZMG9F6yMyM4i1znFPCok",
    "35gCi/gQcF0wR2pE2cpmUEqYFObPf7x7/CEcraZEEqvdve/0r3tBjwUr7MjQXm3",
    "B3TtPjo6S2f5CfzU+WhvdQrly/Bzlv7bPeGLDp+IKYN/MBDqDqBePs5i5l/Ka2z",
    "mEwxKu1dAt8/0n+3xNoTtlWSO5zURRZeztn0cx7pHFc4emsLJe7HKgswAaWmF0N",
    "N+8Rj6efbn//TxLm9LGlLkQGcqwfUmnjwsN7u+4CWRH8Bku2FmnmZLYLJVMjJa4",
    "mgODI/AtcbuhYxtC2lN/FIZ+/inKWbmh7j2VwqBkz7yc9yATcqWrLbXVlsD/+E4",
    "4vXtJUPMQCB9M05m1i28isxWEYf6UssKEgB2lHCXsDyYYtH7X6GwznKybnkhWvF",
    "tKloNwHBgw0ox1wI+uNF5UpI63HlVK3vADFhgwBQ1aOoPPMacefw0xAKwex40Vf",
    "sntaLBDaGoY454t2xYn7qPCJNtq4fi6CScJqHK3RRH13E17Tk7lfc8IxUY4UwIr",
    "eIR0oPv1dAmmuCJqLvCJ/mQ0fDzTKPB+1f5d1BPXXBetcHdsZHsRiEbz1R5xzOT",
    "+meX1EoRTSZliUd/NGx7F9MwzpHrxfEDTJCHbtGwwyfbc6WuzCfRR14riE/g=",
);
// cspell:enable
