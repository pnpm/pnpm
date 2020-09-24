# pkgs-graph

> Create a graph from an array of packages

[![npm version](https://img.shields.io/npm/v/pkgs-graph.svg)](https://www.npmjs.com/package/pkgs-graph)

## Installation

```
<npm|yarn|pnpm> add pkgs-graph
```

## Usage

```js
import createPkgsGraph from 'pkgs-graph'

const {graph} = createPkgsGraph([
  {
    dir: '/home/zkochan/src/foo',
    manifest: {
      name: 'foo',
      version: '1.0.0',
      dependencies: {
        bar: '^1.0.0',
      },
    },
  },
  {
    dir: '/home/zkochan/src/bar',
    manifest: {
      name: 'bar',
      version: '1.1.0',
    },
  }
])

console.log(graph)
//> {
//    '/home/zkochan/src/foo': {
//      dependencies: ['/home/zkochan/src/bar'],
//      manifest: {
//        name: 'foo',
//        version: '1.0.0',
//        dependencies: {
//          bar: '^1.0.0',
//        },
//      },
//    },
//    '/home/zkochan/src/bar': {
//      dependencies: [],
//      manifest: {
//        name: 'bar',
//        version: '1.1.0',
//      },
//    },
//  }
```

## Related

* [find-packages](https://github.com/zkochan/find-packages) - Find all packages inside a directory
* [sort-pkgs](https://github.com/zkochan/sort-pkgs) - Sort packages. Dependents first.

## License

[MIT](LICENSE) © [Zoltan Kochan](https://www.kochan.io)
