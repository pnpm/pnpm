import { graphSequencer } from '../src'

test('graph with three independent self-cycles', () => {
  expect(graphSequencer(new Map([
    ['a', ['a']],
    ['b', ['b']],
    ['c', ['c']],
  ]
  ))).toStrictEqual(
    {
      safe: true,
      chunks: [['a', 'b', 'c']],
      cycles: [
        ['a'], ['b'], ['c'],
      ],
    }
  )
})

test('graph with self-cycle. Sequencing a subgraph', () => {
  expect(graphSequencer(new Map([
    ['a', ['a']],
    ['b', ['b']],
    ['c', ['c']],

  ]), ['a', 'b'])).toStrictEqual(
    {
      safe: true,
      chunks: [['a', 'b']],
      cycles: [['a'], ['b']],
    }
  )
})

test('graph with two self-cycles and an edge linking them', () => {
  expect(graphSequencer(new Map([
    ['a', ['b', 'c']],
    ['b', ['b']],
    ['c', ['b', 'c']]]
  ))).toStrictEqual(
    {
      safe: true,
      chunks: [['b', 'c'], ['a']],
      cycles: [
        ['b'], ['c'],
      ],
    }
  )
})

test('graph with nodes connected to each other sequentially without forming a cycle', () => {
  expect(graphSequencer(new Map([
    ['a', ['b', 'c']],
    ['b', []],
    ['c', ['b']]]
  ))).toStrictEqual(
    {
      safe: true,
      chunks: [['b'], ['c'], ['a']],
      cycles: [],
    }
  )
})

test('graph sequencing with a subset of 3 nodes, ignoring 2 nodes, in a 5-node graph', () => {
  expect(graphSequencer(new Map([
    ['a', ['b', 'c']],
    ['b', []],
    ['c', []],
    ['d', ['a']],
    ['e', ['a', 'b', 'c']]]
  ), ['a', 'd', 'e'])).toStrictEqual(
    {
      safe: true,
      chunks: [['a'], ['d', 'e']],
      cycles: [],
    }
  )
})

test('graph with no edges', () => {
  expect(graphSequencer(new Map([
    ['a', []],
    ['b', []],
    ['c', []],
    ['d', []],
  ]))).toStrictEqual(
    {
      safe: true,
      chunks: [['a', 'b', 'c', 'd']],
      cycles: [],
    }
  )
})

test('graph of isolated nodes with no edges, sequencing a subgraph of selected nodes', () => {
  expect(graphSequencer(new Map([
    ['a', []],
    ['b', []],
    ['c', []],
    ['d', []],
  ]), ['a', 'b', 'c'])).toStrictEqual(
    {
      safe: true,
      chunks: [['a', 'b', 'c']],
      cycles: [],
    }
  )
})

test('graph with multiple dependencies on one item', () => {
  expect(graphSequencer(new Map([
    ['a', ['d']],
    ['b', ['d']],
    ['c', []],
    ['d', []],
  ]))).toStrictEqual(
    {
      safe: true,
      chunks: [['c', 'd'], ['a', 'b']],
      cycles: [],
    }
  )
})

test('graph with resolved cycle', () => {
  expect(graphSequencer(new Map([
    ['a', ['b']],
    ['b', ['c']],
    ['c', ['d']],
    ['d', ['a']],
  ]))).toStrictEqual(
    {
      safe: false,
      chunks: [['a', 'b', 'c', 'd']],
      cycles: [['a', 'b', 'c', 'd']],
    }
  )
})

test('graph with a cycle, but sequencing a subgraph that avoids the cycle', () => {
  expect(graphSequencer(new Map([
    ['a', ['b']],
    ['b', ['c']],
    ['c', ['d']],
    ['d', ['a']],
  ]), ['a', 'b', 'c'])).toStrictEqual(
    {
      safe: true,
      chunks: [['c'], ['b'], ['a']],
      cycles: [],
    }
  )
})

test('graph with resolved cycle with multiple unblocked deps', () => {
  expect(graphSequencer(new Map([
    ['a', ['d']],
    ['b', ['d']],
    ['c', ['d']],
    ['d', ['a']],
  ]))).toStrictEqual(
    {
      safe: false,
      chunks: [
        ['a', 'd'],
        ['b', 'c'],
      ],
      cycles: [['a', 'd']],
    }
  )
})

test('graph with resolved cycle with multiple unblocked deps subgraph', () => {
  expect(graphSequencer(new Map([
    ['a', ['d']],
    ['b', ['d']],
    ['c', ['d']],
    ['d', ['a']],
  ]), ['a', 'b', 'c'])).toStrictEqual(
    {
      safe: true,
      chunks: [
        ['a', 'b', 'c'],
      ],
      cycles: [],
    }
  )
})

test('graph with two cycles', () => {
  expect(graphSequencer(new Map([
    ['a', ['b']],
    ['b', ['a']],
    ['c', ['d']],
    ['d', ['c']],
  ]))).toStrictEqual(
    {
      safe: false,
      chunks: [['a', 'b', 'c', 'd']],
      cycles: [
        ['a', 'b'],
        ['c', 'd'],
      ],
    }
  )
})

test('graph with multiple cycles. case 1', () => {
  expect(graphSequencer(new Map([
    ['a', ['c']],
    ['b', ['a', 'd']],
    ['c', ['b']],
    ['d', ['c', 'e']],
    ['e', []],
  ]))).toStrictEqual(
    {
      safe: false,
      chunks: [['e'], ['a', 'c', 'b'], ['d']],
      cycles: [['a', 'c', 'b']],
    }
  )
})

test('graph with multiple cycles. case 2', () => {
  expect(graphSequencer(new Map([
    ['a', ['b']],
    ['b', ['d']],
    ['c', []],
    ['d', ['b', 'c']],
  ]))).toStrictEqual(
    {
      safe: false,
      chunks: [['c'], ['b', 'd'], ['a']],
      cycles: [['b', 'd']],
    }
  )
})

test('graph with fully connected subgraph and additional connected node', () => {
  expect(graphSequencer(new Map([
    ['a', ['b', 'c', 'd']],
    ['b', ['a', 'c', 'd']],
    ['c', ['a', 'b', 'd']],
    ['d', ['a', 'b', 'c']],
    ['e', ['b']],
  ]))).toStrictEqual(
    {
      safe: false,
      chunks: [['a', 'b', 'c', 'd'], ['e']],
      cycles: [
        ['a', 'b'],
        ['c', 'd'],
      ],
    }
  )
})

test('graph with fully connected subgraph. case 1', () => {
  expect(graphSequencer(new Map([
    ['a', ['b', 'c', 'd']],
    ['b', ['a', 'c', 'd']],
    ['c', ['a', 'b', 'd']],
    ['d', ['a', 'b', 'c']],
    ['e', ['b']],
  ]), ['b', 'e'])).toStrictEqual(
    {
      safe: true,
      chunks: [['b'], ['e']],
      cycles: [],
    }
  )
})

test('graph with fully connected subgraph. case 2', () => {
  expect(graphSequencer(new Map([
    ['a', ['b', 'c', 'd']],
    ['b', ['a', 'c', 'd']],
    ['c', ['a', 'b', 'd']],
    ['d', ['a', 'b', 'c']],
    ['e', ['b']],
  ]), ['a', 'b', 'e'])).toStrictEqual(
    {
      safe: false,
      chunks: [['a', 'b'], ['e']],
      cycles: [['a', 'b']],
    }
  )
})

test('graph with two self-cycles', () => {
  expect(graphSequencer(new Map([
    ['a', ['b', 'c']],
    ['b', ['b']],
    ['c', ['c']],

  ]))).toStrictEqual(
    {
      safe: true,
      chunks: [['b', 'c'], ['a']],
      cycles: [['b'], ['c']],
    }
  )
})

test('graph with two self-cycles. Sequencing a subgraph', () => {
  expect(graphSequencer(new Map([
    ['a', ['b', 'c']],
    ['b', ['b']],
    ['c', ['c']],

  ]), ['b', 'c'])).toStrictEqual(
    {
      safe: true,
      chunks: [['b', 'c']],
      cycles: [['b'], ['c']],
    }
  )
})

test('graph with many nodes', () => {
  expect(graphSequencer(new Map([
    ['a', ['b', 'c']],
    ['b', []],
    ['c', []],
    ['d', ['a']],
    ['e', ['a', 'b', 'c']],
  ]))).toStrictEqual(
    {
      safe: true,
      chunks: [['b', 'c'], ['a'], ['d', 'e']],
      cycles: [],
    }
  )
})

test('graph with many nodes. Sequencing a subgraph', () => {
  expect(graphSequencer(new Map([
    ['a', ['b', 'c']],
    ['b', []],
    ['c', []],
    ['d', ['a']],
    ['e', ['a', 'b', 'c']],
  ]), ['a', 'd', 'e'])).toStrictEqual(
    {
      safe: true,
      chunks: [['a'], ['d', 'e']],
      cycles: [],
    }
  )
})

// TODO: fix this test
test.skip('graph with big cycle', () => {
  expect(graphSequencer(new Map([
    ['a', ['b']],
    ['b', ['a', 'c']],
    ['c', ['a', 'b']],
  ]))).toStrictEqual(
    {
      safe: false,
      chunks: [['a', 'b', 'c']],
      cycles: [['a', 'b', 'c']],
    }
  )
})
