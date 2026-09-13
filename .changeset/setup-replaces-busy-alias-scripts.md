---
"pacquet": patch
---

`pnpm setup` no longer fails with `Text file busy (os error 26)` when writing the `pn` / `pnpx` / `pnx` alias scripts. `$PNPM_HOME/bin` may already hold those names as hardlinks of the pnpm executable — the layout left behind by `npm i -g pnpm` — and truncating one of them in place while that inode is executing raises `ETXTBSY`, which is exactly the case when the process doing the writing is `pnpm setup` itself. The scripts are now written to a sibling temp file and renamed over the target, so the busy inode is left untouched.
