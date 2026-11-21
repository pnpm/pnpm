# WebContainer command inventory

`webcontainer-commands.tsv` is the coverage ledger for the CLI. Its command
paths and aliases are checked against the Clap tree in
`pnpm-cli::wasm_command_inventory`; the test also requires explicit rows for
execution modes that Clap does not model as subcommands.

`coverage` records behavioral evidence from the packaged WebContainer CLI.
`partial` means some command behavior is tested in the real runtime, but the
success and failure cases are incomplete. `routed` means the packaged browser
runtime accepts each command's help path and rejects one unknown global option;
it does not claim the command's behavior works. `none` means neither is tested.

`capability` records the current host assessment:

- `available`: no command-level host restriction is known; this does not mean
  the command has been tested.
- `host-limited`: a working subset exists, while a host feature such as opening
  a browser or running a native tool is unavailable.
- `host-blocked`: the operation depends on a host capability the runtime does
  not provide.
- `implementation-gap`: the host can support the operation, but the WebAssembly
  distribution does not implement it.

Update the ledger when the Clap tree or audited capability changes. Add
behavioral runtime coverage before changing a command from `routed` to
`partial`.

The installed-workspace `clean` and `purge` probes report `Directory not empty`;
the validation record distinguishes these failures from the successful isolated
`clean` probe in [`WEB_CONTAINER_VALIDATION.md`](./WEB_CONTAINER_VALIDATION.md).
