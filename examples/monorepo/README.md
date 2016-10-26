# pnpm usage example in a monorepo 

This is a simple monorepo example with two packages.
The [math](./math) package requires the [sum](./sum) package.

You can see that the `sum` package is specified as a local
dependency in the `math` package's [package.json](./math/package.json).
This is OK, because pnpm will convert the local dependency into a
semver dependency on publish.

## pnpm configuration

Some of the configs are changed in order to make `pnpm` work well with
the monorepo. There is an [.npmrc](./.npmrc) file in the root of the
monorepo with three config values. Lets see what each of them are doing.

**link-local = true**

This is the most important change for a monorepo.
The `link-local` config makes `pnpm` symlink local dependencies and convert them to semver
dependencies before publish. More details about this option at: [usage in monorepos](../../docs/recipes/usage-in-monorepos.md)

**store-path = ~/.store**

This makes `pnpm` use a global store. Packages in monorepos usually have a big set of common
dependencies. A common store will keep all the dependencies in one place, giving a huge boost
to the installation time. More details about shared stores [here](../../docs/recipes/shared-store.md).

**save-exact = false**

Specifying a `save-exact` config in the root of your monorepo is not obligatory but a good thing.
It will guarantee consistency upon how `pnpm publish` will convert the local dependencies into semver dependencies.
Will it use exact or not exact versions.
