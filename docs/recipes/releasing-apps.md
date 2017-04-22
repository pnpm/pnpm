# Releasing apps

There are two ways to release an app with pnpm. One way is to commit `shrinkwrap.yaml` into your repo.
Which we recommend doing anyway. And then in prod you'll have just to run `pnpm install`.
You'll be sure in that case that the same package tree will used, with which you tested your app in other environments.

If you'd like to copy packages to prod, you'll have to commit `shrinkwrap.yaml` anyway. And you'll have to
copy paste the global store or/and the local registry to production. The global store location is configurable
via the `store-path` config key. The local registry location can be changed via the `local-registry` config key.
Then you can run `pnpm install --offline` in your app and pnpm will be using packages that are already in the
global store without making any requests to the npm registry.
