# engine-size

Measures the linked size of pnpm's install engine as an application that
embeds it links it.

```sh
just engine-size                                          # sizes only
just engine-size --registry https://registry.npmjs.org/   # sizes and install timings
```

`--help` lists the other options. `--json <path>` also writes the report as
JSON.

## What it measures

[`engine-size-probe`](../engine-size-probe) is the embedding application: an
executable that loads the configuration of a project directory and runs
`pnpm_package_manager::Install` on it, using only the library API. Add a
dependency or a feature to it only when an embedding application needs it
too, since anything it adds is counted in every size reported here.

For each LTO mode, the tool builds the probe with the workspace's `release`
profile, overriding `lto`, keeping the symbols, and leaving out debug
information. The toolchain is the one in `rust-toolchain.toml`, and the target
is the host. It then reports:

- **Stripped size**: the size of a copy run through `--strip-program`.
- **Sections**: the sections of the stripped copy, largest first.
- **Crates** and **functions**: machine code attributed by symbol. See
  [`attribution.rs`](./src/attribution.rs) for the rules. The numbers are
  estimates: shared generic code goes to the crate that defines it, and
  removing a crate does not remove exactly its share.
- **Install timings**, with `--registry`: the median wall-clock time of a
  frozen-lockfile install of the fixture into an empty store, and again with
  the store populated and `node_modules` removed. The fixture defaults to the
  [integrated benchmark's](../integrated-benchmark/src/fixtures). Each
  install runs in a copy of the fixture directory. Inherited `PNPM_CONFIG_*`
  and `NPM_CONFIG_*` settings are removed, except credentials, proxy, and TLS
  settings. The registry, store, cache, install paths, and
  `ignoreScripts: true` are passed as `PNPM_CONFIG_*` variables, which
  override the fixture's own settings. A public registry makes the cold-store
  timing noisy.

Compare two revisions on the same machine and toolchain. Sizes differ
between targets, and timings between machines.

Crate and function attribution needs a symbol table with function symbols,
which ELF and Mach-O executables have. Windows executables keep their
symbols in a separate PDB file, so attribution fails on Windows.
