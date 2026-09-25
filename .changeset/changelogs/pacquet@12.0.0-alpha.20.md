## 12.0.0-alpha.20

### Patch Changes

- Close three CLI parity gaps with the TypeScript pnpm CLI:

  - `--registry <url>` is now accepted on every command as a universal rc-option, not only through `--config.registry=<url>` (`pnpm view pnpm dist-tags.latest --registry=https://registry.npmjs.org/`).
  - `pnpm add` (and `pnpm add -g`) now accept `--allow-build=<pkg>`, appending the named packages to `allowBuilds` so they can run their lifecycle scripts during the install (`pnpm add @pnpm/exe@11.16.0 --allow-build=@pnpm/exe`).
  - `--dir` / `-C` is now position-independent: it is accepted anywhere on the command line, before or after the subcommand (`pnpm add foo --dir /tmp/proj`).

- `pnpm publish --provenance` now applies the `fetch-timeout` setting to the sigstore signing exchange and retries it up to two more times with exponential backoff when it fails or times out, instead of aborting the publish on the first transient network error or hanging on a stalled connection.

- `pnpm update --latest` now resolves a dependency declared through an `npm:` alias — directly in `package.json` or in the catalog entry a `catalog:` reference points to — to the latest version of the aliased package, keeping the `npm:<name>@` prefix in the rewritten specifier. Previously the alias name itself was looked up on the registry, failing the update with a 404 when no package of that name exists.
