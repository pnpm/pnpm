# Store layout

`pnpm` maintains a flat storage of all your dependencies in `node_modules/.store`. They are then symlinked whereever they're needed.
This is like `npm@2`'s recursive module handling (without the disk space bloat), and like `npm@3`s flat dependency tree (except with each module being predictably atomic).
To illustrate, an installation of [chalk][]@1.1.1 may look like this:

```
.
└─ node_modules/
   ├─ .store/
   |  ├─ store.json
   │  ├─ chalk@1.1.1/_/
   │  │  └─ node_modules/
   │  │     ├─ ansi-styles      -> ../../../ansi-styles@2.1.0/_
   │  │     ├─ has-ansi         -> ../../../has-ansi@2.0.0/_
   │  │     └─ supports-color   -> ../../../supports-color@2.0.0/_
   │  ├─ ansi-styles@2.1.0/_/
   │  ├─ has-ansi@2.0.0/_/
   │  └─ supports-color@2.0.0/_/
   └─ chalk                     -> .store/chalk@1.1.1/_
```

The intermediate `_` directories are needed to hide `node_modules` from npm utilities like `npm ls`, `npm prune`, `npm shrinkwrap` and so on. The name `_` is chosen because it helps make stack traces readable.

[store.json](store-json.md) contains information about all the different internal/external dependencies that the packages in the store have.

[chalk]: https://github.com/chalk/chalk

## Peer dependencies

Symlinks to deep dependencies are stored in `node_modules/.store/node_modules`, in addition to the diagram above. It looks something like this:

```
.
└─ node_modules/
   ├─ .store/
   |  ├─ store.json
   │  ├─ chalk@1.1.1/_/
   │  ├─ ansi-styles@2.1.0/_/
   │  ├─ has-ansi@2.0.0/_/
   │  └─ node_modules/
   │     └─ ansi-styles         -> ../ansi-styles@2.1.0/_
   │     └─ has-ansi            -> ../has-ansi@2.0.0/_
   └─ chalk                     -> .store/chalk@1.1.1/_
```

In the example above, this allows `ansi-styles` to `require('has-ansi')`. This is necessary to make certain packages work, such as [standard][] ([eslint][] + [eslint-config-standard][]).

[standard]: https://github.com/feross/standard
[eslint]: http://eslint.org/
[eslint-config-standard]: https://github.com/feross/eslint-config-standard
