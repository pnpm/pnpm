---
"pacquet": major
---

The global virtual store is now enabled by default. A package is materialized under `<store-dir>/v11/links/` once per machine for each distinct set of dependencies it resolves to, and shared by every project that resolves it the same way, instead of being re-created inside each project's `node_modules/.pnpm`. Installing a dependency another project already installed the same way is close to free, and those projects no longer pay for it twice on disk.

The default is the same in every environment, CI included, so a build never runs against a layout nobody develops against. To opt out, set the setting in `pnpm-workspace.yaml`:

```yaml
enableGlobalVirtualStore: false
```

Because the package directories now live outside the project, a dependency's own dependencies are reachable through symlinks and `NODE_PATH` rather than by walking up the filesystem from a package's real path. Tools that resolve modules by walking directories themselves, instead of asking Node.js to resolve them, may not find a dependency they used to find. Turning the setting off restores the project-local layout.
