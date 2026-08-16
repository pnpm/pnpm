---
"pacquet": major
---

The global virtual store is now enabled by default. Every package is materialized once per machine, under `<store-dir>/v11/links/`, and shared by all the projects that depend on it, instead of being re-created inside each project's `node_modules/.pnpm`. Repeat installs of a dependency another project already installed are close to free, and projects that share dependencies no longer pay for them twice on disk.

The default flips back to off in CI, where the store is usually cold and there is nothing to share yet. Setting `enableGlobalVirtualStore` in `pnpm-workspace.yaml` pins the choice everywhere, CI included:

```yaml
enableGlobalVirtualStore: false
```

Because the package directories now live outside the project, a dependency's own dependencies are reachable through symlinks and `NODE_PATH` rather than by walking up the filesystem from a package's real path. Tools that resolve modules by walking directories themselves, instead of asking Node.js to resolve them, may not find a dependency they used to find. Turning the setting off restores the project-local layout.
