---
id: pkg
title: pnpm pkg
---

Added in: v11.3.0

Manages the contents of `package.json` from the command line.

```sh
pnpm pkg get [<key> [<key> ...]]
pnpm pkg set <key>=<value> [<key>=<value> ...]
pnpm pkg delete <key> [<key> ...]
pnpm pkg fix
```

Nested fields are addressed with dot-separated paths (e.g. `scripts.test`, `repository.url`).

## Commands

### get

Retrieves a value from `package.json`. With no arguments, prints the full manifest. With one or more keys, prints the requested fields.

```sh
pnpm pkg get name
pnpm pkg get name version
pnpm pkg get scripts.test
```

When only one key is requested and it resolves to a string, the raw value is printed; otherwise the value is JSON-encoded. Pass `--json` to always print JSON.

### set

Sets one or more values in `package.json`. Each argument has the `key=value` form.

```sh
pnpm pkg set name=my-package
pnpm pkg set scripts.build="tsc -p ."
pnpm pkg set 'keywords[0]'=cli
```

By default the value is stored as a string. Pass `--json` to parse the value as JSON before storing it (useful for booleans, numbers, arrays, and objects):

```sh
pnpm pkg set private=true --json
pnpm pkg set 'engines={"node":">=22"}' --json
```

### delete

Removes one or more keys from `package.json`.

```sh
pnpm pkg delete scripts.test
pnpm pkg delete keywords
```

### fix

Auto-corrects common errors in `package.json` (e.g. removes a non-string `name` or `version`, drops dependency / `scripts` blocks whose values aren't objects, drops a `bin` field that's neither a string nor an object).

```sh
pnpm pkg fix
```

## Options

### --json

When setting, parses each `value` as JSON before writing. When getting a single key, returns the JSON-encoded form instead of the raw value.

### --recursive, -r

Runs the subcommand on every workspace project, or on every project selected by a `--filter`.

```sh
pnpm -r pkg get name
pnpm -r pkg set version=1.0.0
pnpm --filter "./packages/*" pkg get name
```

`pnpm -r pkg get` returns a JSON object keyed by package name; `set`, `delete`, and `fix` apply to each matched project.
