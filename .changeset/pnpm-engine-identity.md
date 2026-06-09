---
"@pnpm/tools.plugin-commands-self-updater": patch
"pnpm": patch
---

pnpm now verifies the npm registry signature of a package-manager binary before spawning it. When the `packageManager` field (or `pnpm self-update`) makes pnpm download another pnpm version, the staged install is verified corepack-style: the integrity recorded in the staged lockfile must carry a valid npm registry signature for the exact `name@version`, validated against npm's public signing keys that ship embedded in the pnpm CLI. Verification fails closed — a tampered download, an unsigned package, or an unreachable registry refuses the version switch rather than running an unverified binary. It runs only when the wanted version is actually downloaded (a tools-directory cache miss), so repeated commands pay no extra network round trip.
