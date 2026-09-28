---
"pacquet": patch
---

Install lifecycle scripts now resolve node through an inherited anchor when the parent `PATH` holds no node directory. The runner prefers an executable `$NODE`, then `$npm_node_execpath`, then a `node` file along `PATH`, and stamps the result into `NODE` and `npm_node_execpath` for every install spawned script while leaving the rest of the parent env scrubbed as before. With `scriptsPrependNodePath` set to `Always`, the anchor directory is prepended to `PATH`; the `Never` default is unchanged. This fixes clean installs failing with `exec: node: not found` in project `prepare` and workspace `postinstall` scripts when launched with a reduced env under version managers.
