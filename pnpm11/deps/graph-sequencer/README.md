# @pnpm/deps.graph-sequencer

> Sort items in a graph using a topological sort

## Install

```
pnpm add @pnpm/deps.graph-sequencer
```

## Usage

```ts
  expect(graphSequencer(new Map([
    [0, [1]],
    [1, [2]],
    [2, [3]],
    [3, [0]],
  ]), [0, 1, 2, 3])).toStrictEqual(
    {
      safe: false,
      chunks: [[0, 1, 2, 3]],
      cycles: [[0, 1, 2, 3]],
    }
  )
```

## License

[MIT](LICENSE)
