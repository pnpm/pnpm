---
"@pnpm/exe": patch
"pnpm": patch
---

Drop the `darwin-x64` artifact from `@pnpm/exe` and from the GitHub release page. The Node.js SEA mechanism `pnpm pack-app` uses produces a binary that segfaults at startup on Intel Macs because of an upstream Node.js bug ([nodejs/node#62893](https://github.com/nodejs/node/issues/62893), tracked alongside [#59553](https://github.com/nodejs/node/issues/59553); the Node.js team has [opted not to fix it](https://github.com/nodejs/node/pull/60250) on the grounds that x64 macOS is being phased out). Re-signing with `codesign` or `ldid` doesn't help — the corruption is in LIEF's Mach-O surgery, before signing.

Intel Mac users should install pnpm via `npm install -g pnpm` (uses the system Node.js, no SEA), or stay on pnpm 10.x. `@pnpm/exe`'s preinstall on Intel Mac now exits with a clear error pointing at these alternatives.

Closes [#11423](https://github.com/pnpm/pnpm/issues/11423).
