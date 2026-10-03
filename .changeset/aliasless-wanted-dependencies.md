---
"pnpm": patch
"@pnpm/catalogs.resolver": patch
"@pnpm/installing.deps-resolver": patch
"@pnpm/installing.deps-installer": patch
---

Make `WantedDependency.alias` optional to represent selectors whose package name is determined during resolution, such as Git and JSR dependencies. Dependencies read from manifests retain a required alias.
