---
"pnpm": major
---

Switched internal store and cache files from JSON to MessagePack format for improved performance.

This change migrates all internal index files and metadata cache files to use MessagePack serialization instead of JSON. MessagePack provides faster serialization/deserialization and more compact file sizes, resulting in improved installation performance.

Related PR: [#10500](https://github.com/pnpm/pnpm/pull/10500)