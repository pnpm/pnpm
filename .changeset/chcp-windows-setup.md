---
"pacquet": patch
---

Fixed resolving the `chcp` command on Windows during `pnpm setup` by looking for `chcp.com` before `chcp` [pnpm/pnpm#13991](https://github.com/pnpm/pnpm/issues/13991).
