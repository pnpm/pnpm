---
"@pnpm/config.reader": major
"@pnpm/config.commands": major
"@pnpm/auth.commands": minor
"@pnpm/types": minor
"pnpm": major
---

pnpm no longer uses `@pnpm/npm-conf` (a fork of `npm-conf`) to read configuration. Auth and registry settings are now read directly from `.npmrc` files using a purpose-built reader.

### What changed

**pnpm now reads `.npmrc` only for auth and registry settings.** All other settings (like `hoist-pattern`, `node-linker`, `shamefully-hoist`, etc.) must be configured in `pnpm-workspace.yaml` or the global `~/.config/pnpm/config.yaml`.

**The pnpm global config file has been renamed from `rc` to `auth`.** The file at `~/.config/pnpm/rc` is now `~/.config/pnpm/auth`. This file is used by `pnpm login` and `pnpm config set` for storing auth tokens.

**pnpm no longer reads `npm_config_*` environment variables.** Use `pnpm_config_*` environment variables instead (e.g., `pnpm_config_registry` instead of `npm_config_registry`).

**pnpm no longer reads the npm global config** at `$PREFIX/etc/npmrc` or respects the `userconfig`/`globalconfig` override settings from npm.

### Settings that are still read from `.npmrc`

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

### New `npmrcPath` setting

A new `npmrcPath` setting can be added to `pnpm-workspace.yaml` or `~/.config/pnpm/config.yaml` to specify a custom path to the user `.npmrc` file (defaults to `~/.npmrc`):

```yaml
npmrcPath: /custom/path/.npmrc
```

### Auth file read order (highest priority first)

1. `~/.config/pnpm/auth` — pnpm's own auth file
2. `<project>/.npmrc` — project-level
3. `<workspace>/.npmrc` — workspace-level
4. `~/.npmrc` (or custom `npmrcPath`) — user-level fallback

### Migration guide

1. **If you have pnpm settings in `.npmrc`** (like `hoist-pattern`, `node-linker`, `shamefully-hoist`), move them to `pnpm-workspace.yaml`:

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

2. **If you have `~/.config/pnpm/rc`**, rename it to `~/.config/pnpm/auth`:
   ```sh
   mv ~/.config/pnpm/rc ~/.config/pnpm/auth
   ```

3. **If you use `npm_config_*` env vars for auth**, switch to `pnpm_config_*`:
   ```sh
   # Before
   npm_config_registry=https://registry.example.com

   # After
   pnpm_config_registry=https://registry.example.com
   ```

4. **Auth tokens in `~/.npmrc` still work.** No migration needed for registry authentication — pnpm continues to read `~/.npmrc` as a fallback.
