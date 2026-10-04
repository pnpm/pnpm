---
"@pnpm/registry-access.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm unpublish <pkg>@<version>` now deletes the tarball under the registry's path when the registry is served under one, such as Gitea's npm registry. It used to send the delete to the host root and report success without removing the version [#16568](https://github.com/pnpm/pnpm/issues/16568).
