# @pnpm/yaml.document-sync

> Update a YAML document to match the contents of an in-memory object.

<!--@shields('npm')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/yaml.document-sync.svg)](https://www.npmjs.com/package/@pnpm/yaml.document-sync)
<!--/@-->

## Installation

```sh
pnpm add @pnpm/yaml.document-sync
```

## Usage

Given a "_source_" document such as:

```yaml
foo:
  bar:
    # 25 is better than 24
    baz: 25

qux:
  # He was number 1
  - 1
```

And a "_target_" object in-memory:

```ts
import fs from 'node:fs'
import { patchDocument } from '@pnpm/yaml.document-sync'
import yaml from 'yaml'

const source = await fs.promises.readFile('source.yaml', 'utf8')
const document = yaml.parseDocument(source)

const target = {
  foo: { bar: { baz: 25 } },
  qux: [1, 2, 3]
}

patchDocument(document, target)
```

The `patchDocument` function will mutate `document` to match the `target`'s contents, retaining comments along the way. In the example above, the final rendered document will be:

```yaml
foo:
  bar:
    # 25 is better than 24
    baz: 25

qux:
  # He was number 1
  - 1
  - 2
  - 3
```

## Purpose

This package is useful when your codebase:

1. Uses the [yaml](https://www.npmjs.com/package/yaml) library.
2. Calls `.toJSON()` on the parse result and performs changes to it.
3. Needs to "sync" those changes back to the source document.

Instead of this package, consider performing mutations directly on the `yaml.Document` returned from `yaml.parseDocument()` instead. Directly modifying will be faster and more accurate. This package exists as a workaround for codebases that make changes to a JSON object instead and need to reconcile changes back into the source `yaml.Document`.

## Caveats

There are several cases where comment preservation is inherently ambiguous. If the caveats outlined below are problematic, consider modifying the source `yaml.Document` before running the patch function in this package.

### Key Renames

For example, renames of a key are ambiguous. Given:

```yaml
- foo:
    # Test
    bar: 1
```

And a target object to match:

```json
{
  "baz": {
    "bar": 1
  }
}
```

The comment on `bar` won't be retained.

### List Reconciliation

For simple lists (e.g. lists with only primitives), items will be uniquely matched using their contents. However, updates to complex lists are inherently ambiguous.

For example, given a source list with objects as elements:

```yaml
- foo: 1
# Comment
- bar: 2
```

And a target:

```json
[
  { "foo": 1 },
  { "baz": 3 },
  { "bar": 2 }
]
```

The result will erase the comment:

```yaml
- foo: 1
- baz: 3
- bar: 2
```

It's not trivial to detect that the object with `bar` as a field moved down. Detecting this case would require a diffing algorithm, which would be best effort anyway.

Virtual DOM libraries such as React have the same problem. In React, list elements need to specify a `key` prop to uniquely identify each item. This library may take a similar approach in the future if needed. This is not a problem for primitive lists since their values can be compared using simple equality checks.

### Aliases

Given:

```yaml
foo: &config
  - 1
  - 2

bar: *config
```

And a target object:

```json
{
  "foo": [1, 2],
  "bar": [1, 2, 3]
}
```

For correctness, the YAML alias needs to be removed.

```yaml
foo: &config
  - 1
  - 2

bar:
  - 1
  - 2
  - 3
```

## License

MIT
