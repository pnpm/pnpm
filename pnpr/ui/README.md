# pnpr web UI build

Builds the `pnpm.pnpr/apps/pnpr-web` component from
[bit.cloud](https://bit.cloud/pnpm/pnpr) into `../npm/pnpr-ui/dist`, the
payload of `@pnpm/pnpr-ui`. The UI's source lives on bit.cloud, not here.

This directory is a pnpm workspace of its own, because its `@pnpm/*` packages
come from bit.cloud's registry.

To ship a new UI version, update the `@pnpm/pnpr.apps.pnpr-web` pin in
`package.json`, then run `pnpm install` here and commit the lockfile. The
release workflow runs `pnpm install --frozen-lockfile` and `pnpm build` here.

To try a build locally:

```sh
pnpm install
pnpm build
pnpr --config pnpr.yaml   # with `ui: { dir: <repo>/pnpr/npm/pnpr-ui/dist }`
```
