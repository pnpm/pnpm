---
"pacquet": patch
---

`node-linker=hoisted` installs no longer produce a broken layout when a version-conflicted package is depended on by several packages. Its conflicting transitive dependencies were nested under only one of the dependents, so requiring them through any of the other dependents resolved the wrong (root-hoisted) version — for example an ESM `parse-entities@4` resolving `character-entities-legacy@1` instead of `@3`, which crashes with `ERR_IMPORT_ATTRIBUTE_MISSING` on Node.js 22.
