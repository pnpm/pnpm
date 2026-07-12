---
"@pnpm/cli.parse-cli-args": patch
"./pnpm11/pnpm": patch
---

Options that follow `create`, `exec`, or `test` appearing as a subcommand of another command are now parsed instead of being silently treated as positional parameters. For example, `pnpm team create @org:team --registry <url>` previously ignored the `--registry` option and sent the request to the default registry.
