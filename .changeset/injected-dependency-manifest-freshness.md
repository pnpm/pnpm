---
"pacquet": patch
"@pnpm/napi": patch
---

An injected workspace package that lists a dependency also as a peer no longer makes every install resolve again: the dependency is checked against the peer the parent provides, which is what the lockfile records for it.

Repeat installs through the Node-API binding no longer resolve again when a workspace project's `dependencyManifest` differs from its `manifest`. An injected instance of the project is checked against its `dependencyManifest`, and its `workspace:` dependencies against the manifests passed in memory, so project directories need no `package.json`.
