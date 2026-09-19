---
"pacquet": minor
---

Catalog entries can now use the `file:` and `link:` protocols. A relative path in an entry is measured from the directory holding `pnpm-workspace.yaml`, not from the project that references it. A bare path such as `./tarballs/foo.tgz` is measured from there too. It used to resolve against the project that referenced it [#8642](https://github.com/pnpm/pnpm/issues/8642).
