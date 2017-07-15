# dependencies-hierarchy

[![Status](https://travis-ci.org/pnpm/dependencies-hierarchy.svg?branch=master)](https://travis-ci.org/pnpm/dependencies-hierarchy "See test builds")

> Creates a dependencies hierarchy for a symlinked `node_modules`

## Install

Install it via npm.

```
npm install dependencies-hierarchy
```

## API

### default: `dependenciesHierarchy(projectPath, [opts]): Promise<Hierarchy>`

Creates a dependency tree for a project's `node_modules`.

#### Arguments:

* `projectPath` - *String* - The path to the project.
* `[opts.depth]` - *Number* - 0 by default. How deep should the `node_modules` be analyzed.
* `[opts.only]` - *'dev' | 'prod'* - Optional. If set to `dev`, then only packages from `devDependencies` are analyzed.
If set to `prod`, then only packages from `dependencies` are analyzed.

### `forPackages(packageSelectors, projectPath, [opts]): Promise<Hierarchy>`

Creates a dependency tree for a project's `node_modules`. Limits the results to only the paths to the packages named.

#### Arguments:

* `packageSelectors` - *(string | {name: string, version: string})[]* - An array that consist of package names or package names and version ranges.
E.g. `['foo', {name: 'bar', version: '^2.0.0'}]`.
* `projectPath` - *String* - The path to the project
* `[opts.depth]` - *Number* - 0 by default. How deep should the `node_modules` be analyzed.
* `[opts.only]` - *'dev' | 'prod'* - Optional. If set to `dev`, then only packages from `devDependencies` are analyzed.
If set to `prod`, then only packages from `dependencies` are analyzed.

## License

[MIT](LICENSE)
