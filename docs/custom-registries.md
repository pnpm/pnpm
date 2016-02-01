# Custom registries

pnpm follows whatever is configured as npm registries. To use a custom registry, use `npm config`:

```sh
# updates ~/.npmrc
npm config set registry http://npmjs.eu
```

Or to use it for just one command, use environment variables:

```
env npm_registry=http://npmjs.eu pnpm install
```

Private registries are supported, as well.

```sh
npm config set @mycompany:registry https://npm.mycompany.com
pnpm install @mycompany/foo
```
