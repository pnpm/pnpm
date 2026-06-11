---
"@pnpm/config": patch
"pnpm": patch
---

Hardened the warning printed when a project `.npmrc` uses environment variables in registry/auth settings: the suggested `pnpm config set` command is now only included for keys made up of shell-inert characters. Because the key comes from a repository-controlled `.npmrc` and a shell expands `$(...)`, backticks, and `$VAR` even inside double quotes, a crafted key could otherwise have turned the suggested copy-paste command into command execution.
