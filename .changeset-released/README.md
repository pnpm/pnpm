# Released changesets ledger

This directory holds a per-branch record of which changesets have already been released. Each file is named after a release branch (with `/` replaced by `-`, e.g. `main.txt`, `release-10.0.txt`) and contains one consumed changeset id per line.

## Why this exists

Fixes are cherry-picked between `main` and `release/*` branches, so the same changeset file can end up on both. The release branch's release consumes its copy and deletes it, but the cherry-picked copy on `main` would otherwise survive the merge back and be applied a second time on the next `main` release.

The ledger prevents that double-application. Each branch only writes to its own file, so cross-branch merges are conflict-free; the wrapper around `changeset version` reads the union of every file when deciding what to skip.

The directory lives at the repo root (sibling of `.changeset/`) rather than inside `.changeset/` because `@changesets/read` treats every directory inside `.changeset/` as a legacy v1 changeset and tries to read `changes.md` from it.

## How it gets updated

`pnpm bump` runs the wrapper at `__utils__/scripts/src/bump.ts`, which:

1. Reads every `*.txt` in this directory and unions the ids.
2. Hides any `.changeset/<id>.md` whose id is in that union (renames to `<id>.md.released`) so `changeset version` does not see it.
3. Runs `changeset version`, which consumes the remaining `.md` files.
4. Appends the newly-consumed ids to `<current-branch>.txt`.
5. Deletes the hidden files (their consumption is already on record).

If `changeset version` fails, hidden files are renamed back to their original `.md` names so the working tree is left clean.

## Release-PR branch naming

Releases land via a PR rather than a commit pushed straight to the target branch, so the branch the bump runs on is not the branch the release is *for*. Name the release PR branch `release-pr/<target>`, where `<target>` is the branch it merges into (`main`, `release/11.1`, …). The wrapper strips the `release-pr/` prefix and keys the ledger by `<target>`, so every release for `main` accumulates in `main.txt` instead of scattering into a new file per PR.

The target branch is detected from `git rev-parse --abbrev-ref HEAD` (after stripping the prefix); a branch without the prefix is its own target. Set `RELEASE_BRANCH` to override (the prefix is stripped from it too).

## Workflow requirement

Release branches must be merged back into `main` between releases, so that `main` sees the release branch's ledger entries before its own next release runs.
