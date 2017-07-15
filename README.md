# dependencies-hierarchy

> Creates a dependencies hierarchy for a symlinked \`node_modules\`

<!--@shields('npm', 'travis')-->
[![npm version](https://img.shields.io/npm/v/dependencies-hierarchy.svg)](https://www.npmjs.com/package/dependencies-hierarchy) [![Build Status](https://img.shields.io/travis/pnpm/dependencies-hierarchy/master.svg)](https://travis-ci.org/pnpm/dependencies-hierarchy)
<!--/@-->

## Install

Install it via npm.

    npm install dependencies-hierarchy

## API

### default: `dependenciesHierarchy(projectPath, [opts]): Promise<Hierarchy>`

Creates a dependency tree for a project's `node_modules`.

#### Arguments:

- `projectPath` - _String_ - The path to the project.
- `[opts.depth]` - _Number_ - 0 by default. How deep should the `node_modules` be analyzed.
- `[opts.only]` - _'dev' | 'prod'_ - Optional. If set to `dev`, then only packages from `devDependencies` are analyzed.
  If set to `prod`, then only packages from `dependencies` are analyzed.

### `forPackages(packageSelectors, projectPath, [opts]): Promise<Hierarchy>`

Creates a dependency tree for a project's `node_modules`. Limits the results to only the paths to the packages named.

#### Arguments:

- `packageSelectors` - _(string | {name: string, version: string})\[]_ - An array that consist of package names or package names and version ranges.
  E.g. `['foo', {name: 'bar', version: '^2.0.0'}]`.
- `projectPath` - _String_ - The path to the project
- `[opts.depth]` - _Number_ - 0 by default. How deep should the `node_modules` be analyzed.
- `[opts.only]` - _'dev' | 'prod'_ - Optional. If set to `dev`, then only packages from `devDependencies` are analyzed.
  If set to `prod`, then only packages from `dependencies` are analyzed.

## License

[MIT](./LICENSE) © [Zoltan Kochan](https://www.kochan.io/)
