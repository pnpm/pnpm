---
"pacquet": patch
---

Fixed resolution of a direct dependency declared in both `dependencies` and `devDependencies`: the `dependencies` specifier now wins, matching the TypeScript CLI. The `devDependencies` range was resolved instead, recording a lockfile importer entry whose version did not satisfy its specifier — which failed the lockfile up-to-date check and forced a full re-resolve on every install.
