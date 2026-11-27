# Verification with Creusot

[Creusot](https://github.com/creusot-rs/creusot) proves contracts on Rust code: preconditions, postconditions, type invariants, loop invariants, and the absence of panics and overflow. The proofs run as `cargo creusot`, which compiles a crate with the `creusot` cfg set and hands what it proves to the SMT solvers.

The verified crates are the workspace members that take `creusot-std` under `[target.'cfg(creusot)'.dependencies]`. Each crate's root documentation says what is proved about it.

## Running the proofs

Once per machine, on x86_64 Linux with glibc:

```sh
sudo install -d -o "$(id -u)" -g "$(id -g)" /opt/creusot-why3
just install-creusot
export PATH="$HOME/.local/share/creusot/bin:$PATH"
```

`just install-creusot` installs the release `.github/scripts/creusot-prebuilt-install.py` pins, from [creusot-prebuilt](https://github.com/KSXGitHub/creusot-prebuilt), with the nightly it builds with and the SMT solvers. That file is the release's own installer, unchanged, so its checksum matches the release's `SHA256SUMS`. A newer release replaces it.

Then, from the repository root:

```sh
pnpm prove:rust                 # `just creusot`: every verified crate
just creusot --no-cache         # ignore the prover cache
cargo creusot -p <crate>        # one crate
```

`just creusot` fails unless `cargo creusot` proved every Coma file it generated. `cargo creusot` itself exits 0 when `why3find` cannot read them, which a Why3 whose plugins came from two builds causes: install a release into an empty `/opt/creusot-why3`.

A run writes three things at the repository root, all ignored by git: `verif/` holds the generated Coma, `why3find.proofs/` the prover each goal took, and `.why3find/` the prover cache. `why3find.json` configures the provers and is committed.

## How a crate is verified

- **The whole crate is translated.** Creusot proves every function of the crate it is pointed at, and a `PartialEq` impl, a `serde` derive, or an `async fn` there each needs Creusot support of its own. So verified code lives in a small crate with no I/O, which the crates that do I/O depend on.
- **Ordinary builds compile no Creusot.** `creusot-std` is LGPL-2.1-or-later, and `.config/deny.toml` allows it only because no shipped build compiles it. A crate takes it, and `pnpm-creusot-models`, under `[target.'cfg(creusot)'.dependencies]`. Every contract is a `#[cfg_attr(creusot, requires(...))]`, and every item that exists for the proof is `#[cfg(creusot)]`.
- **Logic and lemmas** go in a `spec` module, and properties stated for all inputs in a `theorems` module, both `#[cfg(creusot)]`. A theorem is a verified function whose contract is the property. Proving it is a test that passes on every input at once.
- **Calls into the standard library or a third-party crate** need a contract. `creusot-std` covers much of `std`. The rest goes in `pnpm-creusot-models`, whose crate documentation says how each contract is checked against the function it describes.

## Writing proofs

- Import from `creusot_std::prelude` by name. Its glob also brings a `vec!` that conflicts with the standard one.
- A logic function that reads a private field is `#[logic(open(self))]`. Mark a predicate that is a conjunction `inline` too, so Why3 proves each conjunct as its own goal.
- `proof_assert!` keeps only its last expression as a fact. When it calls a lemma, end it with the lemma's conclusion.
- The solvers rarely find an existential witness. Wrap `exists` in a named predicate with lemmas for each way the program changes it, as `stacked` in `pnpm-strongly-connected-components` does.
- `cargo creusot --dry-run-why3find` prints the `why3find` command it runs. Run that command with `-X` and the failed `.coma` file to print the goal and its context.

## What is trusted

- Every contract in `pnpm-creusot-models`, and the `std` contracts in `creusot-std`.
- Creusot, Why3, and the solvers.
- Termination: Creusot proves a function correct if it returns, unless the function carries `#[check(terminates)]`.
