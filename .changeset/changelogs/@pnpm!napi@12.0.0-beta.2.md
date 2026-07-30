## 12.0.0-beta.2

### Minor Changes

- Added `readConfig(options)`: resolves the configuration the engine's own installs use — registries with their resolved `Authorization` headers, `authHeaderByUri`, proxy, TLS, network limits, store/cache directories, and install behavior settings from the `.npmrc` / `pnpm-workspace.yaml` cascade — so hosts that embed the engine no longer need a JavaScript config reader.
