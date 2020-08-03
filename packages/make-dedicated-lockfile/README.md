# @pnpm/make-dedicated-lockfile

> Creates a dedicated lockfile for a subset of workspace projects

[![npm version](https://img.shields.io/npm/v/@pnpm/make-dedicated-lockfile.svg)](https://www.npmjs.com/package/@pnpm/make-dedicated-lockfile)

## Installation

```sh
<pnpm|npm|yarn> add @pnpm/make-dedicated-lockfile
```

## Usage

Open the directory of the workspace project that you want to create a dedicated lockfile for.

Run `make-dedicated-lockfile` in the terminal.

A new lockfile will be generated, using dependencies from the shared workspace lockfile but
only those that are related to the project in the current working directory or subdirectories.

## License

MIT © [Zoltan Kochan](https://www.kochan.io/)
