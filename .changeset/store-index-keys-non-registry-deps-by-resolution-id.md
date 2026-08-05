---
"pacquet": patch
---

The store index now keys URL, git-host, and `type: git` dependencies by their bare resolution id, matching the key pnpm 11 writes [#13365](https://github.com/pnpm/pnpm/issues/13365). Previously these rows carried a `<name>@` prefix, so a store warmed by one pnpm major was cold for the other and every non-registry dependency was re-downloaded, re-extracted, and re-imported on a switch. A remote tarball also occupied two index rows instead of one, doubling its extraction work.
