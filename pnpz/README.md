# pnpz

Status: proof of concept. Nothing here is released, and every format and
command may change.

pnpz installs system packages (interpreters, databases, command-line tools)
from the NixOS binary cache into a Nix store, without Nix. It never evaluates
the Nix language: a catalog maps a package name and version to the store paths
nixpkgs built for it.

## Try it

```sh
cargo build -p pnpz
alias pnpz="$PWD/target/debug/pnpz"
pnpz add python@3.12 postgresql@16 jq
pnpz run python3 --version
pnpz shell
```

| Command | Does |
| --- | --- |
| `pnpz add <name>[@<version>]...` | Adds packages to `pnpz.yaml`, locks them in `pnpz-lock.yaml`, and installs them. |
| `pnpz install [--frozen]` | Installs what `pnpz.yaml` lists. `--frozen` fails when the lockfile is out of date. |
| `pnpz run <command>...` | Runs a command with the packages on `PATH`, installing them first if needed. |
| `pnpz shell` | Starts `$SHELL` the same way. |

## How it works

| Step | Source |
| --- | --- |
| Name and version to store paths | The catalog, [`src/catalog.rs`](./crates/pnpz/src/catalog.rs). The proof of concept reads the public Nixhub index. |
| Store path to its dependencies | The `.narinfo` documents of the binary cache, followed until the closure is complete. |
| Trust | Each `.narinfo` must carry an ed25519 signature by a trusted key, by default the key of `cache.nixos.org`. The lockfile records each archive's hash. |
| Download | Archives are decompressed (zstd or xz), unpacked, and hashed in one pass, then moved into the store. |
| Running | Binaries from nixpkgs refer to `/nix/store` by absolute path. See [Where the store lives](#where-the-store-lives). |

The Nix formats are in [`crates/nix-store`](./crates/nix-store): store paths,
Nix base32, NAR archives, `.narinfo` documents, and their signatures.

## Where the store lives

pnpz uses the first of these:

1. The directory `PNPZ_STORE_ROOT` names. It takes the place of `/nix`.
2. `/nix`, when `/nix/store` exists and is writable.
3. `$XDG_DATA_HOME/pnpz/nix`, or `~/.local/share/pnpz/nix`.

A store that is not at `/nix` is mounted there for the command `pnpz run`
starts, in a user and mount namespace of its own. This needs no root and no
setup. It is Linux only, and needs a kernel that lets unprivileged users create
user namespaces.

## Settings

| Environment variable | Default |
| --- | --- |
| `PNPZ_STORE_ROOT` | See [Where the store lives](#where-the-store-lives). |
| `PNPZ_CATALOG_URL` | `https://search.devbox.sh` |
| `PNPZ_CACHE_URL` | `https://cache.nixos.org` |
| `PNPZ_TRUSTED_PUBLIC_KEYS` | The key of `cache.nixos.org`. Space-separated `<name>:<base64 key>` entries. |

## Not done

- macOS. The code compiles for it but was never run there, and needs a `/nix`
  the user created.
- Garbage collection, and any record of which projects use a store path.
- Sharing a store with an installed Nix. A `/nix` that pnpz cannot write to is
  hidden inside `pnpz run` by pnpz's own store.
- Libraries for compiling against. Only a package's default outputs are
  installed, which leaves out the `dev` output that holds headers, and the
  environment sets `PATH` only.
- Using the packages outside `pnpz run` and `pnpz shell`, for example from an
  editor, when the store is not at `/nix`.
- Symlinks in the store keep the time they were created at. Nix dates them one
  second after the epoch, like everything else.
- Releases, CI coverage beyond the Rust workspace checks, and documentation.
