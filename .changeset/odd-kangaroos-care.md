---
"@pnpm/parse-cli-args": major
"@pnpm/plugin-commands-script-runners": major
"pnpm": major
---

When using `pnpm run <script>`, all command line arguments after the script name are now passed to the script's argv. Previously flagged arguments (e.g. `--silent`) were intepreted as pnpm arguments unless `--` came before it.
