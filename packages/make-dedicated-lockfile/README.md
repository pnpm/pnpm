# @pnpm/make-dedicated-lockfile

> Creates a dedicated lockfile for a subset of workspace projects

[![npm version](https://img.shields.io/npm/v/@pnpm/make-dedicated-lockfile.svg)](https://www.npmjs.com/package/@pnpm/make-dedicated-lockfile)

**This package is deprecated. Please use the [pnpm deploy] command instead.**

[pnpm deploy]: https://pnpm.io/cli/deploy

## Installation

```sh
pnpm add @pnpm/make-dedicated-lockfile
```

## Usage

Open the directory of the workspace project that you want to create a dedicated lockfile for.

Run `make-dedicated-lockfile` in the terminal.

A new lockfile will be generated, using dependencies from the shared workspace lockfile but
only those that are related to the project in the current working directory or subdirectories.

## License

MIT
