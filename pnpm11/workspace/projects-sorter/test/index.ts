import { expect, test } from '@jest/globals'
import type { ProjectRootDir, ProjectsGraph } from '@pnpm/types'
import { sequenceGraph, sortFilteredProjects, sortProjects } from '@pnpm/workspace.projects-sorter'

function makeGraph (adjacency: Record<string, string[]>): ProjectsGraph {
  const graph: ProjectsGraph = {}
  for (const [dir, dependencies] of Object.entries(adjacency)) {
    graph[dir as ProjectRootDir] = {
      dependencies: dependencies as ProjectRootDir[],
    } as ProjectsGraph[ProjectRootDir]
  }
  return graph
}

// Mirrors how the real selected graph is built: a subset of nodes that keep
// their original `dependencies` arrays (still referencing unselected projects).
function select (graph: ProjectsGraph, names: string[]): ProjectsGraph {
  const selected: ProjectsGraph = {}
  for (const name of names) {
    selected[name as ProjectRootDir] = graph[name as ProjectRootDir]
  }
  return selected
}

const dirs = (...names: string[]): ProjectRootDir[] => names as ProjectRootDir[]

test('sorts every project when only one graph is given', () => {
  const graph = makeGraph({ a: ['b'], b: ['c'], c: [] })
  expect(sortProjects(graph)).toStrictEqual([dirs('c'), dirs('b'), dirs('a')])
})

test('orders selected projects connected only through an unselected project', () => {
  const graph = makeGraph({ a: ['b'], b: ['c'], c: [] })
  expect(sortProjects(select(graph, ['a', 'c']), graph)).toStrictEqual([dirs('c'), dirs('a')])
})

test('keeps independent selected projects in a single chunk', () => {
  const graph = makeGraph({ a: ['b'], b: [], c: [] })
  expect(sortProjects(select(graph, ['a', 'c']), graph)).toStrictEqual([dirs('a', 'c')])
})

test('resolves transitive edges across a diamond of unselected projects', () => {
  const graph = makeGraph({ a: ['x', 'y'], x: ['c'], y: ['c'], c: [] })
  expect(sortProjects(select(graph, ['a', 'c']), graph)).toStrictEqual([dirs('c'), dirs('a')])
})

test('without a full graph, resolution is limited to edges among the sorted projects', () => {
  const graph = makeGraph({ a: ['b'], b: ['c'], c: [] })
  expect(sortProjects(select(graph, ['a', 'c']))).toStrictEqual([dirs('a', 'c')])
})

test('does not reintroduce edges that the selected graph pruned (e.g. prod-only filter)', () => {
  const fullGraph = makeGraph({ a: ['b'], b: [] })
  // The selection dropped a's edge to b (as a prod-only filter drops dev edges).
  const selected = makeGraph({ a: [], b: [] })
  expect(sortProjects(selected, fullGraph)).toStrictEqual([dirs('a', 'b')])
})

test('sortFilteredProjects resolves transitive order through unselected projects for regular filters', () => {
  const fullGraph = makeGraph({ a: ['b'], b: ['c'], c: [] })
  const selected = select(fullGraph, ['a', 'c'])
  expect(sortFilteredProjects({ selectedProjectsGraph: selected, allProjectsGraph: fullGraph }))
    .toStrictEqual([dirs('c'), dirs('a')])
})

test('sortFilteredProjects resolves a prod-only selection through the prod-pruned graph', () => {
  // A prod-only selection sorts through the prod-pruned graph, where b's dev edge
  // to c is gone, so c is not pulled ahead of a.
  const prodGraph = makeGraph({ a: ['b'], b: [], c: [] })
  const fullGraph = makeGraph({ a: ['b'], b: ['c'], c: [] })
  const selected = select(prodGraph, ['a', 'c'])
  expect(sortFilteredProjects({
    selectedProjectsGraph: selected,
    allProjectsGraph: fullGraph,
    prodAllProjectsGraph: prodGraph,
    prodOnlySelectedProjectDirs: dirs('a', 'c'),
  }))
    .toStrictEqual([dirs('a', 'c')])
})

test('sortFilteredProjects orders a prod-only selection by its transitive prod deps', () => {
  // Every edge is a prod edge, so a transitively depends on c through the
  // unselected b and must run after it.
  const prodGraph = makeGraph({ a: ['b'], b: ['c'], c: [] })
  const selected = select(prodGraph, ['a', 'c'])
  expect(sortFilteredProjects({
    selectedProjectsGraph: selected,
    prodAllProjectsGraph: prodGraph,
    prodOnlySelectedProjectDirs: dirs('a', 'c'),
  }))
    .toStrictEqual([dirs('c'), dirs('a')])
})

test('sortFilteredProjects keeps prod-only roots on the prod graph in mixed selections', () => {
  const fullGraph = makeGraph({ a: ['b'], b: ['c'], c: ['x'], x: ['a'], d: [] })
  const prodGraph = makeGraph({ a: ['b'], b: ['c'], c: ['x'], x: [], d: [] })
  const selected = {
    ...select(prodGraph, ['a', 'c']),
    ...select(fullGraph, ['d']),
  }
  expect(sortFilteredProjects({
    selectedProjectsGraph: selected,
    allProjectsGraph: fullGraph,
    prodAllProjectsGraph: prodGraph,
    prodOnlySelectedProjectDirs: dirs('a', 'c'),
  })).toStrictEqual([dirs('c', 'd'), dirs('a')])
})

test('detects a cycle that passes through unselected projects', () => {
  const graph = makeGraph({ a: ['b'], b: ['c'], c: ['a'] })
  expect(sequenceGraph(select(graph, ['a', 'c']), graph).safe).toBe(false)
})
