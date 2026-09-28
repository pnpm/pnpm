## 1100.0.0

### Patch Changes

- `@pnpm/config.env-replace`, `@pnpm/log.group`, `@pnpm/network.agent`, `@pnpm/network.ca-file`, `@pnpm/network.config`, `@pnpm/network.proxy-agent`, and the `@pnpm/os.env.path-extender` packages are now published from the pnpm repository. They are ES modules.

- `pickSettingByUrl` now returns a matching setting whose value is falsy, such as `false` or an empty string. It also matches a URL that has both a port and a query string against the settings of its path.
