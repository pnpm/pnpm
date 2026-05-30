//! Serializer options that match pnpm's lockfile YAML formatting.
//!
//! pnpm's `pnpm-lock.yaml` is emitted by `js-yaml` with default options, which
//! keeps long single-line scalars (notably `integrity: sha512-…` hashes) as
//! plain scalars. `serde_saphyr` defaults to folding long single-line strings
//! into block style (`>-`), so we explicitly disable that to preserve byte-
//! level parity with what pnpm produces.

use serde::Serialize;
use serde_saphyr::{SerializerOptions, ser, ser_options, to_string_with_options};

/// Serializer options matching pnpm's lockfile output.
fn options() -> SerializerOptions {
    ser_options! {
        prefer_block_scalars: false,
    }
}

/// Serialize `value` to a YAML string with options that match pnpm's lockfile
/// formatting.
pub(crate) fn to_string<Value: Serialize>(value: &Value) -> Result<String, ser::Error> {
    to_string_with_options(value, options())
}
