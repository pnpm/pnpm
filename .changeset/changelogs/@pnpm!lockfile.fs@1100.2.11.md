## 1100.2.11

### Patch Changes

- `pnpm install` no longer re-resolves an up-to-date lockfile on every run when a patched package is a peer in a peer cycle [#16418](https://github.com/pnpm/pnpm/issues/16418).

- Workspaces now work with a dependency or project named `constructor`, a project directory named `__proto__`, and injected packages that contain files named like `constructor` or `valueOf`. pnpm crashed on some of these names and silently skipped others.
