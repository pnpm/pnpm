---
"@pnpm/global.commands": patch
"pnpm": patch
---

Fix `pnpm -g ls --json` and `pnpm -g ls --parseable` so they emit valid JSON and parseable output respectively, matching pnpm 10 behavior. Since the isolated global packages refactor in pnpm 11, the global list command had a custom path that always printed plain text and ignored `--json`/`--parseable`, which broke tools like `npm-check-updates` that parse the JSON output [#11440](https://github.com/pnpm/pnpm/issues/11440).

`pnpm -g ls --depth=<n>` (with n > 0) now errors when more than one isolated global install would be involved, since each install has its own lockfile and merging their transitive trees would be incoherent. When the request can be narrowed to a single install group, the regular `list` flow is used and the full dependency tree is shown.
