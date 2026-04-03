---
"@pnpm/config.reader": major
"@pnpm/config.commands": major
"@pnpm/auth.commands": minor
"@pnpm/types": minor
"pnpm": major
---

pnpm no longer reads all settings from `.npmrc`. Only auth and registry settings are read from `.npmrc` files. All other settings (like `hoist-pattern`, `node-linker`, `shamefully-hoist`, etc.) must be configured in `pnpm-workspace.yaml` or the global `~/.config/pnpm/config.yaml`.

### What changed

**`.npmrc` is now only for auth and registry settings.** pnpm-specific settings in `.npmrc` are ignored. Move them to `pnpm-workspace.yaml`.

**pnpm no longer reads `npm_config_*` environment variables.** Use `pnpm_config_*` environment variables instead (e.g., `pnpm_config_registry` instead of `npm_config_registry`).

**pnpm no longer reads the npm global config** at `$PREFIX/etc/npmrc`.

**`pnpm login` writes auth tokens** to `~/.config/pnpm/auth.ini`.

### Settings still read from `.npmrc`

The following settings continue to be read from `.npmrc` files (project-level and `~/.npmrc`):

- `registry` and `@scope:registry` — registry URLs
- `//registry.example.com/:_authToken` — auth tokens per registry
- `_auth`, `_authToken`, `_password`, `username`, `email` — global auth credentials
- `//registry.example.com/:tokenHelper` — token helper commands
- `ca`, `cafile`, `cert`, `key`, `certfile`, `keyfile` — SSL certificates
- `strict-ssl` — SSL verification
- `proxy`, `https-proxy`, `no-proxy` — proxy settings
- `local-address` — local network address binding
- `git-shallow-hosts` — git shallow clone hosts

### New `npmrcAuthFile` setting

A new `npmrcAuthFile` setting can be added to `pnpm-workspace.yaml` or `~/.config/pnpm/config.yaml` to specify a custom path to the user `.npmrc` file (defaults to `~/.npmrc`):

```yaml
npmrcAuthFile: /custom/path/.npmrc
```

### New `registries` setting in `pnpm-workspace.yaml`

Registry URLs can now be configured in `pnpm-workspace.yaml`, so there's no need to commit `.npmrc` files with registry mappings:

```yaml
registries:
  default: https://registry.npmjs.org/
  "@my-org": https://private.example.com/
  "@internal": https://nexus.corp.com/
```

This replaces the `.npmrc` settings `registry=...` and `@scope:registry=...`.

### Auth file read order (highest priority first)

1. `~/.config/pnpm/auth.ini` — pnpm's own auth file (written by `pnpm login`)
2. `<workspace>/.npmrc` — workspace root (or project root)
3. `~/.npmrc` (or custom `npmrcAuthFile`) — user-level fallback

Note: `.npmrc` is only read from the workspace root, not from individual package directories.

### Migration guide

1. **Move pnpm settings from `.npmrc` to `pnpm-workspace.yaml`:**

   Before (`.npmrc`):
   ```ini
   shamefully-hoist=true
   node-linker=hoisted
   ```

   After (`pnpm-workspace.yaml`):
   ```yaml
   shamefullyHoist: true
   nodeLinker: hoisted
   ```

2. **Move scoped registry mappings from `.npmrc` to `pnpm-workspace.yaml`:**

   Before (`.npmrc`):
   ```ini
   @my-org:registry=https://private.example.com
   ```

   After (`pnpm-workspace.yaml`):
   ```yaml
   registries:
     "@my-org": https://private.example.com/
   ```

3. **If you use `npm_config_*` env vars**, switch to `pnpm_config_*`:
   ```sh
   # Before
   npm_config_registry=https://registry.example.com

   # After
   pnpm_config_registry=https://registry.example.com
   ```

4. **Auth tokens in `~/.npmrc` still work.** No migration needed for registry authentication — pnpm continues to read `~/.npmrc` as a fallback.
