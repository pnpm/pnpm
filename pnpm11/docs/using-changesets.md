---
id: using-changesets
title: Using Changesets with pnpm
---

:::tip

Since v11.13.0, pnpm can manage workspace releases natively, without the Changesets CLI. It reads and writes the same `.changeset/*.md` files. See [Release management](./versioning.md).

Since v11.16.0, [`pnpm update --changeset`](./cli/update.md#--changeset) can also write a changeset for the dependency bumps an update makes.

:::

:::note

At the time of writing this documentation, the latest pnpm version was
v10.4.1. The latest [Changesets](https://github.com/changesets/changesets) version was v2.28.0.

:::

## Setup

To setup changesets on a pnpm workspace, install changesets as a dev dependency
in the root of the workspace:

```sh
pnpm add -Dw @changesets/cli
```

Then run changesets' init command to generate a changesets config:

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
2. Run `pnpm install --no-frozen-lockfile`. This will update the lockfile and rebuild packages.
3. Commit the changes.
4. Run `pnpm publish -r`. This command will publish all packages that have
   bumped versions not yet present in the registry.

## Integration with GitHub Actions

To automate the process, you can use `changeset version` with GitHub actions. The action will detect when changeset files arrive in the `main` branch, and then open a new PR listing all the packages with bumped versions. The PR will automatically update itself every time a new changeset file arrives in `main`. Once merged the packages will be updated, and if the `publish` input has been specified on the action they will  be published using the given command.

### Add a publish script

Add a new script called `ci:publish` which executes `pnpm publish -r`. This will publish to the registry once the PR created by `changeset version` has been merged. If the package is public and scoped, adding `--access=public` may be necessary to prevent npm rejecting the publish.

```json title="package.json"
{
   "scripts": {
      "ci:publish": "pnpm publish -r"
   },
   ...
}
```

### Add the workflow

Add a new workflow at `.github/workflows/changesets.yml`. This workflow will create a new branch and PR, so Actions should be given **read and write** permissions in the repo settings (`github.com/<repo-owner>/<repo-name>/settings/actions`). If including the `publish` input on the `changesets/action` step, the repo should also include an auth token for npm as a repository secret named `NPM_TOKEN`.

```yaml title=".github/workflows/changesets.yml"
name: Changesets

on:
  push:
    branches:
      - main

env:
  CI: true

jobs:
  version:
    timeout-minutes: 15
    runs-on: ubuntu-latest
    steps:
      - name: Checkout code repository
        uses: actions/checkout@v4

      - name: Setup pnpm and Node.js
        uses: pnpm/setup@v2
        with:
          runtime: node@20
          cache: true

      - name: Create and publish versions
        uses: changesets/action@v1
        with:
          commit: "chore: update versions"
          title: "chore: update versions"
          publish: pnpm ci:publish
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          NPM_TOKEN: ${{ secrets.NPM_TOKEN }}
```

More info and documentation regarding the changesets action can be found
[here](https://github.com/changesets/action).
