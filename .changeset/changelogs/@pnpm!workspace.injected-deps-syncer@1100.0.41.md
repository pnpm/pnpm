## 1100.0.41

### Patch Changes

- Workspaces now work with a dependency or project named `constructor`, a project directory named `__proto__`, and injected packages that contain files named like `constructor` or `valueOf`. pnpm crashed on some of these names and silently skipped others.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.35
