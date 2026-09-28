## 1100.0.0

### Patch Changes

- `@pnpm/config.env-replace`, `@pnpm/log.group`, `@pnpm/network.agent`, `@pnpm/network.ca-file`, `@pnpm/network.config`, `@pnpm/network.proxy-agent`, and the `@pnpm/os.env.path-extender` packages are now published from the pnpm repository. They are ES modules.

- `getAgent` and `getProxyAgent` no longer return a cached agent created with different `strictSsl`, `maxSockets`, or `timeout` settings. An omitted `strictSsl` used to reuse an agent created with `strictSsl: false`, which skipped certificate verification.

- Updated dependencies:
  - @pnpm/error@1100.2.1
