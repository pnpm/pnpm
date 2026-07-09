---
"@pnpm/config.parse-overrides": minor
"@pnpm/hooks.read-package-hook": minor
"@pnpm/installing.deps-installer": minor
"pnpm": minor
---

Added a new override selector form with an empty range — `"pkg@": "<version>"` — called a convergence override. It rewrites a dependency edge only when its exact version satisfies the edge's declared range, so compatible consumers converge on one version while incompatible consumers keep their own resolution — now and for any dependent added in the future [#12794](https://github.com/pnpm/pnpm/issues/12794).

```yaml
overrides:
  "form-data@": 4.0.6
```

The value must be an exact version. When a full resolution detects that every declared range also admits a newer version, pnpm warns that the override is stale and names the version to converge on. Previously an empty range in an override selector was undocumented and behaved like a bare (unscoped) override.
