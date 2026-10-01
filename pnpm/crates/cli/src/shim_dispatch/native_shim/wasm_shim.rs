use std::{io, path::Path};

pub(super) fn publish(_source: &Path, executable: &Path) -> io::Result<()> {
    let runtime = std::env::var("PNPM_WASM_RUNTIME").map_err(io::Error::other)?;
    let artifact = std::env::var("PNPM_WASM_ARTIFACT").map_err(io::Error::other)?;
    let runtime = serde_json::to_string(&runtime).map_err(io::Error::other)?;
    let artifact = serde_json::to_string(&artifact).map_err(io::Error::other)?;
    let script = format!(
        r"#!/usr/bin/env node
(async () => {{
  const {{ pathToFileURL }} = await import('node:url');
  const {{ runWasm }} = await import(pathToFileURL({runtime}).href);
  process.exitCode = await runWasm({artifact}, {{
    args: process.argv.slice(1), executable: process.argv[1]
  }});
}})().catch(error => {{ console.error(error); process.exitCode = 1; }});
"
    );
    crate::executable_link::replace_script(script.as_bytes(), executable)
}
