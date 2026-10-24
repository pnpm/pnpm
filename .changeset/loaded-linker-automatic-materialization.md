---
"pacquet": patch
---

With `nodeLinker.type: loaded`, packages that need a build no longer have to be listed in `nodeLinker.excluded`. A package with a matching side-effects cache entry loads from the store. A cache miss, a package with a native `.node` addon, and their dependencies are materialized automatically.
