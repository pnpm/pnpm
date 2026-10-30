---
"pacquet": minor
---

With `nodeLinker.type: loaded`, packages that need a build no longer have to be listed in `nodeLinker.excluded`. A package with a matching side-effects cache entry loads from the store. When side-effects cache reads and writes are enabled, an approved build without a cache entry runs in a temporary directory, and its output loads from the store. Other cache misses, packages with native `.node` addons, packages whose JavaScript reads the filesystem or its own location, and their dependencies are materialized automatically.
