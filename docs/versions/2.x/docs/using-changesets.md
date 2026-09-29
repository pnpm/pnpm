---
id: using-changesets
title: Using Changesets with pnpm
---

> At the time of writing this documentation, the latest pnpm version was
> v5.17.3. The latest Changesets version was v2.14.1.

## Setup

To setup changesets on a pnpm workspace, install changesets as a dev dependency
in the root of the workspace:

```sh
pnpm add -DW @changesets/cli
```

Then changesets' init command:

```sh
pnpx changeset init
```

## Adding new changesets

To generate a new changeset, run `pnpx changeset` in the root of the repository.
The generated markdown files in the `.changeset` directory should be committed
to the repository.

## Releasing changes

1. Run `pnpx changeset version`. This will bump the versions of the packages
previously specified with `pnpx changeset` (and any dependents of those) and
update the changelog files.
2. Run `pnpm install`. This will update the lockfile and rebuild packages.
3. Commit the changes.
4. Run `pnpm publish -r`. This command will publish all packages that have
bumped versions not yet present in the registry.

## Using GitHub Actions

To automate the process, you can use `changeset version` with GitHub actions.

In a nutshell, the action will detect new changesets on the `master` branch,
apply them, commit the updated metadata and changelogs, and open a Pull Request.
You could also publish your packages automatically.

More info and documentation regarding this action can be found
[here](https://github.com/changesets/action).

```yaml
name: Changesets
on:
  push:
    branches:
      - master
env:
  CI: true
  PNPM_CACHE_FOLDER: .pnpm-store
jobs:
  version:
    timeout-minutes: 15
    runs-on: ubuntu-latest
    steps:
      - name: checkout code repository
        uses: actions/checkout@v2
        with:
          fetch-depth: 0
      - name: setup node.js
        uses: actions/setup-node@v2
        with:
          node-version: 14
      - name: install pnpm
        run: npm i pnpm@latest -g
      - name: setup pnpm config
        run: pnpm config set store-dir $PNPM_CACHE_FOLDER
      - name: install dependencies
        run: pnpm install
      - name: create versions
        uses: changesets/action@master
        with:
          version: pnpm ci:version
          commit: 'chore: update versions'
          title: 'chore: update versions'
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
```
