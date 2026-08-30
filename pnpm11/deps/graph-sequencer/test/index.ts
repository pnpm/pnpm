import { expect, test } from '@jest/globals'

import { graphSequencer } from '../src/index.js'

test('graph with three independent self-cycles', () => {
  expect(graphSequencer(new Map([
    ['a', ['a']],
    ['b', ['b']],
    ['c', ['c']],
  ]
  ))).toStrictEqual(
    {
      order: ['a', 'b', 'c'],
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
      order: ['a', 'b'],
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
      order: ['b', 'c', 'a'],
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
      order: ['b', 'c', 'a'],
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
      order: ['a', 'd', 'e'],
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
      order: ['a', 'b', 'c', 'd'],
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
      order: ['a', 'b', 'c'],
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
      order: ['c', 'd', 'a', 'b'],
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
      order: ['a', 'b', 'c', 'd'],
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
      order: ['c', 'b', 'a'],
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
      order: ['a', 'd', 'b', 'c'],
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
      order: ['a', 'b', 'c'],
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
      order: ['a', 'b', 'c', 'd'],
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
      order: ['e', 'a', 'c', 'b', 'd'],
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
      order: ['c', 'b', 'd', 'a'],
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
      order: ['a', 'b', 'c', 'd', 'e'],
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
      order: ['b', 'e'],
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
      order: ['a', 'b', 'e'],
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
      order: ['b', 'c', 'a'],
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
      order: ['b', 'c'],
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
      order: ['b', 'c', 'a', 'd', 'e'],
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
      order: ['a', 'd', 'e'],
      cycles: [],
    }
  )
})

test('graph with big cycle', () => {
  expect(graphSequencer(new Map([
    ['a', ['b']],
    ['b', ['a', 'c']],
    ['c', ['a', 'b']],
  ]))).toStrictEqual(
    {
      order: ['a', 'b', 'c'],
      cycles: [['a', 'b', 'c']],
    }
  )
})

test('graph with three cycles', () => {
  expect(graphSequencer(new Map([
    ['a', ['b']],
    ['b', ['a', 'c']],
    ['c', ['a', 'b']],
    ['e', ['f']],
    ['f', ['e']],
    ['g', ['g']],
  ]))).toStrictEqual(
    {
      order: ['a', 'b', 'c', 'e', 'f', 'g'],
      cycles: [['a', 'b', 'c'], ['e', 'f'], ['g']],
    }
  )
})

// A dense chain where every node depends on its nine predecessors guards the
// O(V + E) rewrite: the quadratic full scan took seconds at workspace scale
// (https://github.com/pnpm/pnpm/issues/14149).
test('deep chain sorts in linear time', () => {
  const count = 20_000
  const names = Array.from({ length: count }, (_, i) => `project-${i.toString().padStart(5, '0')}`)
  const graph = new Map<string, string[]>(
    names.map((name, i) => [name, names.slice(Math.max(0, i - 9), i)])
  )
  const startedAt = performance.now()
  const result = graphSequencer(graph, names)
  const elapsedMs = performance.now() - startedAt
  expect(result.cycles).toStrictEqual([])
  expect(result.order).toHaveLength(count)
  expect(result.order[0]).toBe(names[0])
  expect(result.order[count - 1]).toBe(names[count - 1])
  expect(elapsedMs).toBeLessThan(5000)
})

// Many nodes lead into one large ring and are listed before it. The
// component filter must keep the cycle pass from paying a full ring walk
// per dependent, and the ring stays before every dependent.
test('dependents of a cycle sort in linear time', () => {
  const ringLen = 3_000
  const dependentCount = 30_000
  const dependents = Array.from({ length: dependentCount }, (_, i) => `dep-${i.toString().padStart(5, '0')}`)
  const ring = Array.from({ length: ringLen }, (_, i) => `ring-${i.toString().padStart(4, '0')}`)
  const graph = new Map<string, string[]>()
  for (const [i, name] of dependents.entries()) {
    graph.set(name, [ring[i % ringLen]])
  }
  for (const [i, name] of ring.entries()) {
    graph.set(name, [ring[(i + 1) % ringLen]])
  }
  const included = [...dependents, ...ring]
  const startedAt = performance.now()
  const result = graphSequencer(graph, included)
  const elapsedMs = performance.now() - startedAt
  expect(result.cycles.some((cycle) => cycle.length > 1)).toBe(true)
  expect(result.cycles).toHaveLength(1)
  expect(result.order).toHaveLength(ringLen + dependentCount)
  expect(new Set(result.order.slice(0, ringLen))).toStrictEqual(new Set(ring))
  expect(new Set(result.order.slice(ringLen))).toStrictEqual(new Set(dependents))
  expect(elapsedMs).toBeLessThan(5000)
})

// Thousands of two-node rings, each pointing into the next. Confining the
// cycle search to a ring's own strongly connected component keeps one pass
// from walking every downstream ring per cycle.
test('chained components sort in linear time', () => {
  const ringCount = 5_000
  const names: Array<[string, string]> = Array.from({ length: ringCount }, (_, i) => [`a-${i.toString().padStart(4, '0')}`, `b-${i.toString().padStart(4, '0')}`])
  const graph = new Map<string, string[]>()
  for (const [i, [a, b]] of names.entries()) {
    const aEdges = [b]
    if (i + 1 < ringCount) {
      aEdges.push(names[i + 1][0])
    }
    graph.set(a, aEdges)
    graph.set(b, [a])
  }
  const included = names.flatMap(([a, b]) => [a, b])
  const startedAt = performance.now()
  const result = graphSequencer(graph, included)
  const elapsedMs = performance.now() - startedAt
  expect(result.cycles.some((cycle) => cycle.length > 1)).toBe(true)
  expect(result.cycles).toHaveLength(ringCount)
  expect(result.order).toHaveLength(ringCount * 2)
  expect(elapsedMs).toBeLessThan(5000)
})
