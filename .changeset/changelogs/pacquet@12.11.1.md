## 12.11.1

This release fixes two ways `pnpm install` could fail, runs tools of any Rust release through `pnx`, and treats `registry.npmjs.com` as an alias of the npm registry.

### Patch Changes

- `pnpm install` no longer fails when `packageManager` pins the pnpm version that is already running and the registry does not publish that version. pnpm warns and continues. A registry mirror that has not synced a release no longer blocks the commands of a project pinned to it.

- `pnpm install` no longer fails with `ERR_PNPM_CMD_SHIM_CHMOD` when `node_modules/.bin` holds a shim that another user created, if everyone can already execute it and it is not world-writable. This happens when several users share one checkout.

- `enable-modules-dir=false` (`enableModulesDir: false` through the Node.js addon) fetches the registry packages the host can install into the store again, as pnpm v10 did, while still writing nothing under `node_modules`. The setting exists for a `node_modules` that something else mounts from the store, such as a FUSE daemon, and that consumer no longer has to download each package on first access. A plain `--lockfile-only` run still fetches nothing.

- `pnpm pack` and `pnpm publish` no longer put `.npmignore` and `.gitignore` files in the tarball. A `files` entry that names one still ships it.

- pnpm now treats `https://registry.npmjs.com/` as an alias of `https://registry.npmjs.org/`. Registry requests, credentials, and trusted publishing use the canonical hostname.

- `pnx --package=rust@<channel> <tool>` runs a tool of that Rust release, for example `pnx --package=rust@nightly-2026-01-01 cargo build`. pnpm installs the release with the components and targets from `rust-toolchain.toml` and the target of each `--target` argument.

- `pnpm install` now links agent skills for more coding agents. It detects the agent from `ANTIGRAVITY_AGENT`, `COPILOT_AGENT`, `COPILOT_CLI`, `CODEX_THREAD_ID`, `CODEX_SANDBOX`, `AI_AGENT`, and `CLAUDE_CODE` [pnpm/tasks#116](https://github.com/pnpm/tasks/issues/116).

- When the registry rejects `pnpm stage publish`, the error message now starts with "Failed to stage package".
