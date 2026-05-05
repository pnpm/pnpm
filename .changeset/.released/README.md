# Released changesets ledger

This directory holds a per-branch record of which changesets have already been released. Each file is named after a release branch (with `/` replaced by `-`, e.g. `main.txt`, `release-10.0.txt`) and contains one consumed changeset id per line.

## Why this exists

Fixes are cherry-picked between `main` and `release/*` branches, so the same changeset file can end up on both. The release branch's release consumes its copy and deletes it, but the cherry-picked copy on `main` would otherwise survive the merge back and be applied a second time on the next `main` release.

The ledger prevents that double-application. Each branch only writes to its own file, so cross-branch merges are conflict-free; the wrapper around `changeset version` reads the union of every file when deciding what to skip.

## How it gets updated

`pnpm bump` runs the wrapper at `__utils__/scripts/src/bump.ts`, which:

1. Reads every `*.txt` in this directory and unions the ids.
2. Hides any `.changeset/<id>.md` whose id is in that union (renames to `<id>.md.released`) so `changeset version` does not see it.
3. Runs `changeset version`, which consumes the remaining `.md` files.
4. Appends the newly-consumed ids to `<current-branch>.txt`.
5. Deletes the hidden files (their consumption is already on record).

If `changeset version` fails, hidden files are renamed back to their original `.md` names so the working tree is left clean. The current branch is detected from `git rev-parse --abbrev-ref HEAD`; set `RELEASE_BRANCH` to override.

## Workflow requirement

Release branches must be merged back into `main` between releases, so that `main` sees the release branch's ledger entries before its own next release runs.
