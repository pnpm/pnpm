---
id: node-distributions
title: Node.js distributions
---

Use a Node.js upstream to download stable runtimes and native addon headers through pnpr.
Import a registry with `pnpr registries import`:

```yaml title="registries.yaml"
registries:
  runtimes:
    type: upstream
    ecosystem: node
    public: true
defaultRegistry:
  node: runtimes
```

The upstream defaults to `https://nodejs.org/dist/`. Set `url` to use another stable release origin.
Read and cache grants apply to the package name `node`.

Set the pnpm download mirror in the global configuration:

```yaml title="config.yaml"
tools:
  node:
    mirror: https://pnpr.example.com/node
```

For node-gyp, set the distribution URL in the build environment:

```sh
export npm_config_disturl=https://pnpr.example.com/node/release/
```

To select a named registry, use `/node/~runtimes` as the mirror and
`/node/~runtimes/release/` as the node-gyp distribution URL.

pnpr caches the release index, checksums, and detached signatures. It checks
runtime archives, headers, and Windows import libraries against the release's
`SHASUMS256.txt` before adding them to the artifact cache.
