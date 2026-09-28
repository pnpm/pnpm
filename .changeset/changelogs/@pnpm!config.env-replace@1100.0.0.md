## 1100.0.0

### Patch Changes

- A `${VAR}` placeholder in `.npmrc` or `pnpm-workspace.yaml` whose name matches a built-in object property, such as `${toString}`, is now treated as an unset variable. It used to be replaced with the source text of a JavaScript function.

- `@pnpm/config.env-replace`, `@pnpm/log.group`, `@pnpm/network.agent`, `@pnpm/network.ca-file`, `@pnpm/network.config`, `@pnpm/network.proxy-agent`, and the `@pnpm/os.env.path-extender` packages are now published from the pnpm repository. They are ES modules.
