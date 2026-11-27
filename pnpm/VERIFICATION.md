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

`just install-creusot` installs the release of [creusot-prebuilt](https://github.com/KSXGitHub/creusot-prebuilt) that `pnpm/scripts/install-creusot.mjs` pins, with the nightly it builds with and the SMT solvers. The script pins every file it downloads by SHA-256 and refuses one that hashes to anything else:

- The release's archives, downloaded directly from the release.
- The nightly's `rustc`, `rust-std` and `cargo` components. The script installs them itself, replacing that nightly's directory under `~/.rustup/toolchains`, rather than asking rustup for them.
- The solvers, which `creusot-install` downloads. It carries their hashes, and it is one of the pinned archives.

Moving to a newer release means copying the archive table from the release's `release.json`, and the component hashes from Rust's channel manifest for the nightly's date. `just install-creusot --downloads <dir>` keeps each verified download in `<dir>`, and reuses a kept file only while it still matches its pin.

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

## Passing a fact between functions

A fact about one value travels in the value's type, not in a precondition that every consumer restates.

- Wrap the value in a type with private fields, and state the fact as the type's `impl Invariant`. Creusot proves the invariant wherever code passes the value to a function or returns it, and wherever a `&mut` borrow of it ends. Every verified function that takes the value assumes it.
- Give the type one constructor whose `#[requires]` is the fact, so the code that builds the value proves the fact once, at that call.
- Keep that constructor private. Only `cargo creusot` checks a precondition, so a caller in another crate could build the type with the fact false. Code outside the crate gets the type from a function that establishes the fact itself, as `split_name_suffix` does.
- Mark the invariant `#[logic(open)]` and state it over public logic functions rather than the private fields, so a verified crate downstream can read it. A logic function that reads a field stays `open(crate)`.
- A consumer or a theorem takes the type instead of a precondition. `written_key_reads_back` takes a `KeyName` and states nothing about the name.
- A fact that relates several values, such as a key being a name and a suffix joined by `@`, stays a `#[requires]`, since no one argument can carry it.

`KeyName` and `Scope` in `pnpm-pkg-name-split` are examples, and so is `StronglyConnectedComponents`.

## Writing proofs

- Import from `creusot_std::prelude` by name. Its glob also brings a `vec!` that conflicts with the standard one.
- A logic function that reads a private field is `#[logic(open(self))]`. Mark a predicate that is a conjunction `inline` too, so Why3 proves each conjunct as its own goal.
- `proof_assert!` keeps only its last expression as a fact. When it calls a lemma, end it with the lemma's conclusion.
- `str` methods take byte offsets and the contracts speak of characters. `to_bytes` relates the two, and `injective_to_bytes` shows that two strings with equal bytes are equal. The `spec` module of `pnpm-pkg-name-split` holds lemmas for slicing a `str` at the byte length of a substring.
- The solvers rarely find an existential witness. Wrap `exists` in a named predicate with lemmas for each way the program changes it, as `stacked` in `pnpm-strongly-connected-components` does.
- Prove a new or changed proof cold before pushing it: remove `target/creusot`, `verif/`, `why3find.proofs` and `.why3find`, then run `just creusot`. `why3find.proofs` is not committed, so CI meets a new goal without the hint an earlier local run left, and a goal the solvers found once by search can fail there. Halving `time` in `why3find.json` for that run shows the goals close to the limit.
- Creusot does not know what a string literal holds, so `text == ".."` proves nothing about `text`. Classify the string by its characters instead, as `segment_kind` in `pnpm-relative-path` does with `split_first_char`.
- Re-associating `concat` is a step the solvers can miss. Cite `Seq::concat_assoc` for it rather than leaving it to search.
- `cargo creusot --dry-run-why3find` prints the `why3find` command it runs. Run that command with `-X` and the failed `.coma` file to print the goal and its context.

## What is trusted

- Every contract in `pnpm-creusot-models`, and the `std` contracts in `creusot-std`.
- Creusot, Why3, and the solvers.
- Termination: Creusot proves a function correct if it returns, unless the function carries `#[check(terminates)]`.
