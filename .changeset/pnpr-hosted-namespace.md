---
"@pnpm/pnpr": patch
---

pnpr refuses hosted stores and metadata databases created by earlier alpha releases. Replace `registries.<name>.org` with `registries.<name>.namespace` before starting the upgraded server. Preserve the old hosted store and metadata database, including npm package maintainer lists. Point pnpr at an empty hosted store and metadata database before upgrading. The new store starts a fresh transparency log that does not extend earlier checkpoints. Republish hosted packages after upgrading. Republishing an npm package makes the publisher its sole maintainer. Restore any additional npm package maintainers after republishing.

Hosted registries with a `namespace` now store archives under `namespaces/<namespace>/<ecosystem>/`. Hosted registries without a `namespace` keep using `default/<ecosystem>/`. SQL metadata now uses the `namespaces` table and `namespace` column.

`artifacts.owners` replaces `artifacts.orgs` for shared-artifact, compiler-cache, and pipeline-run access rules.

Publication responses, package release-note listings, and `pnpr gc` audit spans now report `namespace` [zkochan/pnpm-tasks#66](https://github.com/zkochan/pnpm-tasks/issues/66).
