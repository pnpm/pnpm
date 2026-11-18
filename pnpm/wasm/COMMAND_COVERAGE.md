# WebContainer command inventory

`webcontainer-commands.tsv` is the coverage ledger for the CLI. Its command
paths and aliases are checked against the Clap tree in
`pnpm-cli::wasm_command_inventory`; the test also requires explicit rows for
execution modes that Clap does not model as subcommands.

`coverage` records behavioral evidence from the packaged WebContainer CLI.
`partial` means some real-runtime behavior is covered, not that success and
failure cases are complete. `none` means no such coverage is recorded.

`capability` records the current host assessment:

- `available`: no command-level host restriction is known; this does not mean
  the command has been tested.
- `host-limited`: a working subset exists, while a host feature such as opening
  a browser or running a native tool is unavailable.
- `host-blocked`: the operation depends on a host capability the runtime does
  not provide.
- `implementation-gap`: the host can support the operation, but the WebAssembly
  distribution does not implement it.
- `not-implemented`: pnpm does not implement the command in its native CLI.

Update the ledger when the Clap tree or audited capability changes. Add
packaged-runtime coverage before changing a command from `none` to `partial`.
