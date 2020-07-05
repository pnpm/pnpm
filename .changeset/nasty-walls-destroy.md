---
"@pnpm/parse-cli-args": major
---

New required option added: `knownCommands`.

Any unknown command is assumed to be a script. So `pnpm foo` becomes `pnpm run foo`.
