import path from 'node:path'

import { describe, expect, test } from '@jest/globals'
import type { Project, ProjectRootDir, ProjectsGraph } from '@pnpm/types'

import { selectProjectsToVerify } from '../src/selectProjectsToVerify.js'

const workspaceDir = path.resolve('/workspace')

describe('selectProjectsToVerify', () => {
  test('adds the workspace dependencies of the selected projects transitively', async () => {
    const allProjectsGraph = createGraph({ a: ['b'], b: ['c'], c: [], d: [] })
    expect(await selectNames({
      filter: ['a'],
      allProjectsGraph,
      selectedProjectsGraph: pickGraph(allProjectsGraph, ['a']),
    })).toStrictEqual(['a', 'b', 'c'])
  })

  test('reads a project selected only by --filter-prod from the prod graph', async () => {
    const allProjectsGraph = createGraph({ a: ['b'], b: [] })
    const prodAllProjectsGraph = createGraph({ a: [], b: [] })
    expect(await selectNames({
      filterProd: ['a'],
      allProjectsGraph,
      prodAllProjectsGraph,
      prodOnlySelectedProjectDirs: [dirOf('a')],
      selectedProjectsGraph: pickGraph(prodAllProjectsGraph, ['a']),
    })).toStrictEqual(['a'])
  })

  test('leaves out a dependency that a negated selector excludes from the install', async () => {
    const allProjectsGraph = createGraph({ a: ['b'], b: ['c'], c: [] })
    expect(await selectNames({
      filter: ['!b'],
      allProjectsGraph,
      selectedProjectsGraph: pickGraph(allProjectsGraph, ['a', 'c']),
    })).toStrictEqual(['a', 'c'])
  })

  test('keeps a selected project the install selection does not reach', async () => {
    // `root` is selected by --workspace-root, which adds no selector to the
    // install's filter arguments.
    const allProjectsGraph = createGraph({ root: [], a: ['b'], b: [] })
    expect(await selectNames({
      filter: ['a', '!b'],
      allProjectsGraph,
      selectedProjectsGraph: pickGraph(allProjectsGraph, ['a', 'root']),
    })).toStrictEqual(['a', 'root'])
  })

  test('resolves a path selector where the install the gate spawns runs', async () => {
    // The install runs in `dir`, where `!../b` names b.
    const allProjectsGraph = createGraph({ a: ['b'], b: [] })
    expect(await selectNames({
      dir: dirOf('a'),
      filter: ['!../b'],
      allProjectsGraph,
      selectedProjectsGraph: pickGraph(allProjectsGraph, ['a']),
    })).toStrictEqual(['a'])
  })

  test('returns the selection as it is without a workspace graph', async () => {
    const selectedProjectsGraph = createGraph({ a: ['b'] })
    expect(await selectProjectsToVerify({ selectedProjectsGraph })).toBe(selectedProjectsGraph)
    expect(await selectProjectsToVerify({})).toBeUndefined()
  })
})

function dirOf (name: string): ProjectRootDir {
  return path.join(workspaceDir, name) as ProjectRootDir
}

function createGraph (adjacency: Record<string, string[]>): ProjectsGraph {
  return Object.fromEntries(Object.entries(adjacency).map(([name, dependencies]) => [
    dirOf(name),
    {
      dependencies: dependencies.map(dirOf),
      package: { rootDir: dirOf(name), manifest: { name } } as unknown as Project,
    },
  ]))
}

function pickGraph (graph: ProjectsGraph, names: string[]): ProjectsGraph {
  return Object.fromEntries(names.map((name) => [dirOf(name), graph[dirOf(name)]]))
}

async function selectNames (opts: Parameters<typeof selectProjectsToVerify>[0]): Promise<string[]> {
  const selected = await selectProjectsToVerify({ workspaceDir, ...opts })
  return Object.keys(selected ?? {}).map((dir) => path.basename(dir)).sort()
}
