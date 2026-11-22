---
"pacquet": patch
---

`pnpm cache list`, `view`, and `delete` search across registries by default. Pass `--registry` to restrict them to one registry.

`pnpm find-hash` reports an unknown hash when the store is empty. Inspection commands with `--json` write errors to stdout. `pnpm doctor` checks the resolved global bin directory on PATH.

`pnpm list -r --no-sort --reverse` reverses the selected project order. Recursive runs honor `--sequential` with `--parallel`. Changed-file ignore patterns support negation.

`pnpm clean` forwards positional arguments to a project's clean script. Shell completion uses the cursor position when editing an existing command.
