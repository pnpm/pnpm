---
"pacquet": minor
---

Added a `preferredManifestFormat` setting to `pnpm-workspace.yaml`, selecting which manifest format wins in a project directory that holds more than one — for example a `package.json5` carrying the real manifest next to a stub `package.json` kept for tools that cannot read JSON5.

```yaml
# pnpm-workspace.yaml
preferredManifestFormat: json5  # 'json' (default) | 'json5' | 'yaml'
```

The default is unchanged. A preference only reorders the existing `package.json` > `package.json5` > `package.yaml` chain, so a directory without the preferred format is still resolved through the remaining ones and no project is hidden. It governs the manifests a workspace owns, never those of its dependencies [#3027](https://github.com/pnpm/pnpm/issues/3027) [#5541](https://github.com/pnpm/pnpm/issues/5541).
