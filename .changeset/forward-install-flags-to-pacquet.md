---
"@pnpm/installing.commands": patch
"pnpm": patch
---

When the install engine is delegated to pacquet via `configDependencies`, the user's CLI flags passed to `pnpm install` (e.g. `--no-runtime`, `--prod`, `--dev`, `--no-optional`, `--node-linker`, `--cpu`/`--os`/`--libc`, `--offline`, `--prefer-offline`) are now forwarded to pacquet's `install` subcommand verbatim. Previously pacquet was invoked with a fixed argument list, so flags like `--no-runtime` were silently dropped. Flag forwarding is gated on the command being `install`/`i`; `add`, `update`, and `dedupe` still don't forward (their flag surface doesn't line up with pacquet's `install`).
