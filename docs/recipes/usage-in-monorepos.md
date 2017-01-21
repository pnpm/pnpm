# Usage in monorepos

`pnpm`'s usage of a global store makes it a perfect choice
for monorepo projects. But there are even more benefits you'll get!

When you work in a monorepo, you need all your dependencies linked together via `npm link`.
However, you also need them to be required by each other in their `package.json`.

This is really inconvenient for a few reasons:

1. having the dependencies required as semver dependencies, will download them from the registry
during installation.
2. `npm link` prevents downloading from the registry. However, all the dependencies has to
be manually linked between each other.

`pnpm` solves these issues with a special option called `link-local`. When the `link-local`
config value is set to `true`, `pnpm` will do to changes in it's usual behavior:

1. local dependencies will be resolved with symlinks instead of copy/pasting
2. before publish, the local dependencies will be converted into semver dependencies

Here is an [example monorepo](https://github.com/rstacruz/pnpm/tree/master/examples/monorepo) to see how it works.
