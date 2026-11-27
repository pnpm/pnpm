#!/usr/bin/env bash
# Install the Creusot release `.github/scripts/creusot-prebuilt-install.py`
# pins, the nightly it builds with, and the SMT solvers it proves with.
#
# The release's Why3 has its prefix compiled in, so the directory the
# release names, `/opt/creusot-why3`, has to exist and be writable first.
# `cargo creusot` is then on PATH once `$CREUSOT_DATA_HOME/bin` is.
set -euo pipefail

installer=.github/scripts/creusot-prebuilt-install.py
python3 "$installer" cargo-creusot creusot-rustc creusot-install creusot-prelude why3 why3find
channel=$(python3 "$installer" --list | sed -n 's/.* built against //p')
rustup toolchain install "$channel" --profile minimal --no-self-update
data=${CREUSOT_DATA_HOME:-${XDG_DATA_HOME:-$HOME/.local/share}/creusot}
CREUSOT_DATA_HOME=$data "$data/bin/creusot-install" provers
