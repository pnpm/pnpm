---
id: using-changesets
title: Using Changesets with pnpm
---

:::note

At the time of writing this documentation, the latest pnpm version was
v6.14. The latest [Changesets](https://github.com/changesets/changesets) version was v2.16.0.

:::

## Setup

To setup changesets on a pnpm workspace, install changesets as a dev dependency
in the root of the workspace:

```sh
pnpm add -Dw @changesets/cli
```

Then changesets' init command:

```sh
pnpm changeset init
```

## Adding new changesets

To generate a new changeset, run `pnpm changeset` in the root of the repository.
The generated markdown files in the `.changeset` directory should be committed
to the repository.

## Releasing changes

1. Run `pnpm changeset version`. This will bump the versions of the packages
   previously specified with `pnpm changeset` (and any dependents of those) and
   update the changelog files.
2. Run `pnpm install`. This will update the lockfile and rebuild packages.
3. Commit the changes.
4. Run `pnpm publish -r`. This command will publish all packages that have
   bumped versions not yet present in the registry.

## Using GitHub Actions

To automate the process, you can use `changeset version` with GitHub actions.

### Bump up package versions

The action will detect when changeset files arrive in the `main` branch, and then open a new PR listing all the packages with bumped versions. Once merged, the packages will be updated and you can decide whether to publish or not by adding the `publish` property.

### Publishing

Add a new script `ci:publish` which executes `pnpm publish -r`.
It will publish to the registry once the PR is opened by `changeset version`.

**package.json**
```json
{
   "scripts": {
      "ci:publish": "pnpm publish -r"
   },
   ...
}
```

```yaml
name: Changesets
on:
  push:
    branches:
      - main
env:
  CI: true
  PNPM_CACHE_FOLDER: .pnpm-store
jobs:
  version:
    timeout-minutes: 15
    runs-on: ubuntu-latest
    steps:
      - name: checkout code repository
        uses: actions/checkout@v3
        with:
          fetch-depth: 0
      - name: setup node.js
        uses: actions/setup-node@v3
        with:
          node-version: 14
      - name: install pnpm
        run: npm i pnpm@latest -g
      - name: Setup npmrc
        run: echo "//registry.npmjs.org/:_authToken=${{ secrets.NPM_TOKEN }}" > .npmrc
      - name: setup pnpm config
        run: pnpm config set store-dir $PNPM_CACHE_FOLDER
      - name: install dependencies
        run: pnpm install
      - name: create and publish versions
        uses: changesets/action@v1
        with:
          version: pnpm ci:version
          commit: "chore: update versions"
          title: "chore: update versions"
          publish: pnpm ci:publish
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
```

More info and documentation regarding this action can be found
[here](https://github.com/changesets/action).
