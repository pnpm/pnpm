---
"@pnpm/config.normalize-registries": minor
"@pnpm/store.connection-manager": minor
"@pnpm/resolving.npm-resolver": minor
"@pnpm/config.reader": minor
"@pnpm/types": minor
"pacquet": minor
"pnpm": minor
---

A registry can now declare that its abbreviated metadata carries the `time` field, so `resolutionMode: time-based` reads the full metadata document only from the registries that need it:

```yaml
resolutionMode: time-based
registries:
  https://npm.internal.example/:
    supportsTimeField: true
```

`registry.npmjs.org` omits `time` from abbreviated metadata, so a time-based resolution has to fall back to the much larger full document. That fallback used to be all-or-nothing: `registrySupportsTimeField` answered for every registry at once, so a project resolving from both the public registry and a Verdaccio instance either paid for full metadata everywhere or claimed a `time` field npmjs does not serve. The answer is now per registry, and `registrySupportsTimeField` remains the answer for every registry that does not declare one.

The declaration is also sent to a pnpr server, which applies it to the resolution it runs on the client's behalf.
