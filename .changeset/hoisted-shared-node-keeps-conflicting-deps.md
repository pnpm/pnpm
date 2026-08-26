---
"pacquet": patch
---

`node-linker=hoisted` installs no longer produce broken layouts on graphs with version conflicts. Three hoister fixes, aligning with `@yarnpkg/nm` (which the TypeScript CLI delegates to):

- A version-conflicted package depended on by several packages kept its conflicting transitive dependencies under only one of the dependents, so requiring them through any other dependent resolved the wrong (root-hoisted) version — for example an ESM `parse-entities@4` resolving `character-entities-legacy` v1 instead of v3, which crashes with `ERR_IMPORT_ATTRIBUTE_MISSING` on Node.js 22. Hoist decisions are now made per parent path on decoupled copies (ports upstream's `decoupleGraphNode`).
- Peer-resolution variants of one package version now collapse onto a single copy (ports pnpm v11's `depPathByPkgId` mapping) instead of conflict-nesting a copy under every dependent — on peer-variant-heavy graphs (such as `bit`'s) the old behavior also made the per-path walk explode.
- Hoisting no longer shadows names a subtree resolves through an ancestor directory: a candidate is refused when a nearer ancestor holds a different version of its name (upstream's "filled by parent" scan) or when the hoist root's subtree already resolves that name from above (upstream's `usedDependencies` gate).
