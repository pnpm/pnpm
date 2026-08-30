import { expect, test } from '@jest/globals'
import { graphSequencer } from '@pnpm/deps.graph-sequencer'
import type { ProjectRootDir, ProjectsGraph } from '@pnpm/types'
import { filteredProjectsDependencies, projectsDependencies, sequenceGraph } from '@pnpm/workspace.projects-sorter'

const dirs = (...names: string[]): ProjectRootDir[] => names as ProjectRootDir[]

test('projectsDependencies orders every project after its dependencies', () => {
  const graph = makeGraph({ a: ['b'], b: ['c'], c: [] })
  expect(graphOrder(projectsDependencies(graph))).toStrictEqual(dirs('c', 'b', 'a'))
})

test('projectsDependencies ignores dependencies on projects absent from the graph', () => {
  const graph = makeGraph({ a: ['b'], b: ['c'], c: [] })
  expect(graphOrder(projectsDependencies(select(graph, ['a', 'c'])))).toStrictEqual(dirs('a', 'c'))
})

test('sequenceGraph does not report a self-referencing project as a cycle', () => {
  expect(sequenceGraph(makeGraph({ a: ['a'] })).cycles).toStrictEqual([])
})

test('sequenceGraph flags a dependency cycle as unsafe', () => {
  expect(sequenceGraph(makeGraph({ a: ['b'], b: ['a'] })).cycles.some((cycle) => cycle.length > 1)).toBe(true)
})

test('orders selected projects connected only through an unselected project', () => {
  const fullGraph = makeGraph({ a: ['b'], b: ['c'], c: [] })
  expect(graphOrder(filteredProjectsDependencies({ selectedProjectsGraph: select(fullGraph, ['a', 'c']), allProjectsGraph: fullGraph })))
    .toStrictEqual(dirs('c', 'a'))
})

test('without a full graph, leaves edges to unselected projects unresolved', () => {
  // No allProjectsGraph, so a's edge to the unselected b cannot be tunneled.
  expect(graphOrder(filteredProjectsDependencies({ selectedProjectsGraph: makeGraph({ a: ['b'], c: [] }) })))
    .toStrictEqual(dirs('a', 'c'))
})

test('keeps independent selected projects in selection order', () => {
  const fullGraph = makeGraph({ a: ['b'], b: [], c: [] })
  expect(graphOrder(filteredProjectsDependencies({ selectedProjectsGraph: select(fullGraph, ['a', 'c']), allProjectsGraph: fullGraph })))
    .toStrictEqual(dirs('a', 'c'))
})

test('resolves transitive edges across a diamond of unselected projects', () => {
  const fullGraph = makeGraph({ a: ['x', 'y'], x: ['c'], y: ['c'], c: [] })
  expect(graphOrder(filteredProjectsDependencies({ selectedProjectsGraph: select(fullGraph, ['a', 'c']), allProjectsGraph: fullGraph })))
    .toStrictEqual(dirs('c', 'a'))
})

test('does not reintroduce edges that the selected graph pruned (e.g. prod-only filter)', () => {
  const fullGraph = makeGraph({ a: ['b'], b: [] })
  // The selection dropped a's edge to b (as a prod-only filter drops dev edges).
  const selected = makeGraph({ a: [], b: [] })
  expect(graphOrder(filteredProjectsDependencies({ selectedProjectsGraph: selected, allProjectsGraph: fullGraph })))
    .toStrictEqual(dirs('a', 'b'))
})

test('filteredProjectsDependencies resolves a prod-only selection through the prod-pruned graph', () => {
  // A prod-only selection sorts through the prod-pruned graph, where b's dev edge
  // to c is gone, so c is not pulled ahead of a.
  const prodGraph = makeGraph({ a: ['b'], b: [], c: [] })
  const fullGraph = makeGraph({ a: ['b'], b: ['c'], c: [] })
  const selected = select(prodGraph, ['a', 'c'])
  expect(graphOrder(filteredProjectsDependencies({
    selectedProjectsGraph: selected,
    allProjectsGraph: fullGraph,
    prodAllProjectsGraph: prodGraph,
    prodOnlySelectedProjectDirs: dirs('a', 'c'),
  })))
    .toStrictEqual(dirs('a', 'c'))
})

test('filteredProjectsDependencies orders a prod-only selection by its transitive prod deps', () => {
  // Every edge is a prod edge, so a transitively depends on c through the
  // unselected b and must run after it.
  const prodGraph = makeGraph({ a: ['b'], b: ['c'], c: [] })
  const selected = select(prodGraph, ['a', 'c'])
  expect(graphOrder(filteredProjectsDependencies({
    selectedProjectsGraph: selected,
    prodAllProjectsGraph: prodGraph,
    prodOnlySelectedProjectDirs: dirs('a', 'c'),
  })))
    .toStrictEqual(dirs('c', 'a'))
})

test('filteredProjectsDependencies keeps prod-only roots on the prod graph in mixed selections', () => {
  const fullGraph = makeGraph({ a: ['b'], b: ['c'], c: ['x'], x: ['a'], d: [] })
  const prodGraph = makeGraph({ a: ['b'], b: ['c'], c: ['x'], x: [], d: [] })
  const selected = {
    ...select(prodGraph, ['a', 'c']),
    ...select(fullGraph, ['d']),
  }
  expect(graphOrder(filteredProjectsDependencies({
    selectedProjectsGraph: selected,
    allProjectsGraph: fullGraph,
    prodAllProjectsGraph: prodGraph,
    prodOnlySelectedProjectDirs: dirs('a', 'c'),
  }))).toStrictEqual(dirs('c', 'd', 'a'))
})

test('does not order a regular filter across a dev edge pruned by a prod-only selection', () => {
  // a -> x -> c -> d with x selected prod-only. x's edge to c is a dev edge the
  // prod selection drops, so it is absent from the ordering graph; a reaches c
  // only through it and therefore stays concurrent with c rather than after it.
  const fullGraph = makeGraph({ a: ['x'], x: ['c'], c: ['d'], d: [] })
  const prodGraph = makeGraph({ a: ['x'], x: [], c: ['d'], d: [] })
  const selected = {
    ...select(prodGraph, ['x']),
    ...select(fullGraph, ['a', 'c', 'd']),
  }
  expect(graphOrder(filteredProjectsDependencies({
    selectedProjectsGraph: selected,
    allProjectsGraph: fullGraph,
    prodAllProjectsGraph: prodGraph,
    prodOnlySelectedProjectDirs: dirs('x'),
  }))).toStrictEqual(dirs('x', 'd', 'a', 'c'))
})

test('orders a regular filter across a prod edge kept by a prod-only selection', () => {
  // Same shape, but x -> c is a prod edge the prod graph keeps, so the full
  // a -> x -> c -> d chain holds even though x is sorted through the prod graph.
  const fullGraph = makeGraph({ a: ['x'], x: ['c'], c: ['d'], d: [] })
  const prodGraph = makeGraph({ a: ['x'], x: ['c'], c: ['d'], d: [] })
  const selected = {
    ...select(prodGraph, ['x']),
    ...select(fullGraph, ['a', 'c', 'd']),
  }
  expect(graphOrder(filteredProjectsDependencies({
    selectedProjectsGraph: selected,
    allProjectsGraph: fullGraph,
    prodAllProjectsGraph: prodGraph,
    prodOnlySelectedProjectDirs: dirs('x'),
  }))).toStrictEqual(dirs('d', 'c', 'x', 'a'))
})

test('detects a cycle that passes through unselected projects', () => {
  const graph = makeGraph({ a: ['b'], b: ['c'], c: ['a'] })
  const dependencies = filteredProjectsDependencies({ selectedProjectsGraph: select(graph, ['a', 'c']), allProjectsGraph: graph })
  const result = graphSequencer(dependencies, [...dependencies.keys()])
  expect(result.cycles.some((cycle) => cycle.length > 1)).toBe(true)
  expect(new Set(result.order)).toStrictEqual(new Set(dirs('a', 'c')))
})

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

function graphOrder (graph: Map<ProjectRootDir, ProjectRootDir[]>): ProjectRootDir[] {
  return graphSequencer(graph, [...graph.keys()]).order
}
