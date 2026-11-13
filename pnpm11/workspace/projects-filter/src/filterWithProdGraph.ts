import type { ProjectRootDir } from '@pnpm/types'
import type { BaseProject } from '@pnpm/workspace.projects-graph'
import { partition } from 'ramda'

import type { FilterWorkspaceProjectsOptions, ProjectGraph } from './index.js'
import type { ProjectSelector } from './parseProjectSelector.js'

export interface FilteredGraphWithProdResult<Pkg extends BaseProject> {
  selectedProjectsGraph: ProjectGraph<Pkg>
  prodOnlySelectedProjectDirs?: ProjectRootDir[]
  unmatchedFilters: string[]
}

export type FilterGraphRunner = <Pkg extends BaseProject>(
  projectsGraph: ProjectGraph<Pkg>,
  opts: FilterWorkspaceProjectsOptions,
  projectSelectors: ProjectSelector[]
) => Promise<{
  selected: ProjectRootDir[]
  unmatchedFilters: string[]
}>

interface SelectionOrigin {
  prodMatched: boolean
  prodOnly: boolean
}

interface SelectorChunk {
  exclude: boolean
  selectors: ProjectSelector[]
}

interface ChunkEvaluationResult {
  prodSelected: ProjectRootDir[]
  allSelected: ProjectRootDir[]
  unmatchedFilters: string[]
}

export async function runFilterWithProdGraph<Pkg extends BaseProject> (
  graphs: {
    allProjectsGraph: ProjectGraph<Pkg>
    prodProjectsGraph?: ProjectGraph<Pkg>
    filterGraph: FilterGraphRunner
  },
  projectSelectors: ProjectSelector[],
  opts: FilterWorkspaceProjectsOptions
): Promise<FilteredGraphWithProdResult<Pkg>> {
  const { allProjectsGraph, prodProjectsGraph, filterGraph } = graphs
  if (projectSelectors.length === 0) {
    return {
      selectedProjectsGraph: allProjectsGraph,
      unmatchedFilters: [],
    }
  }

  const chunks = groupSelectorsIntoChunks(projectSelectors)
  const selected = createInitialSelection(allProjectsGraph, prodProjectsGraph, chunks[0])
  const unmatchedFilters: string[] = []
  const effectiveProdGraph = prodProjectsGraph ?? allProjectsGraph

  for (const chunk of chunks) {
    // eslint-disable-next-line no-await-in-loop -- include and exclude chunks apply in order, so each result depends on the ones before it
    const evalResult = await evaluateSelectorChunk(
      { allProjectsGraph, prodProjectsGraph: effectiveProdGraph, filterGraph },
      opts,
      chunk
    )
    unmatchedFilters.push(...evalResult.unmatchedFilters)
    applyChunkResult(selected, chunk.exclude, evalResult)
  }

  return finalizeSelectedProjectsGraph({ allProjectsGraph, prodProjectsGraph }, selected, unmatchedFilters)
}

function groupSelectorsIntoChunks (projectSelectors: ProjectSelector[]): SelectorChunk[] {
  const chunks: SelectorChunk[] = []
  for (const selector of projectSelectors) {
    const last = chunks[chunks.length - 1]
    if (last && last.exclude === Boolean(selector.exclude)) {
      last.selectors.push(selector)
    } else {
      chunks.push({
        exclude: Boolean(selector.exclude),
        selectors: [selector],
      })
    }
  }
  return chunks
}

function createInitialSelection<Pkg extends BaseProject> (
  allProjectsGraph: ProjectGraph<Pkg>,
  prodProjectsGraph: ProjectGraph<Pkg> | undefined,
  firstChunk: SelectorChunk
): Map<ProjectRootDir, SelectionOrigin> {
  const selected = new Map<ProjectRootDir, SelectionOrigin>()
  if (!firstChunk.exclude) return selected
  const prodSeeded = prodProjectsGraph != null && firstChunk.selectors.every((selector) => Boolean(selector.followProdDepsOnly))
  for (const dir of Object.keys(allProjectsGraph) as ProjectRootDir[]) {
    selected.set(dir, { prodMatched: prodSeeded, prodOnly: prodSeeded })
  }
  return selected
}

async function evaluateSelectorChunk<Pkg extends BaseProject> (
  graphs: {
    allProjectsGraph: ProjectGraph<Pkg>
    prodProjectsGraph: ProjectGraph<Pkg>
    filterGraph: FilterGraphRunner
  },
  opts: FilterWorkspaceProjectsOptions,
  chunk: SelectorChunk
): Promise<ChunkEvaluationResult> {
  const [prodSelectors, allSelectors] = partition(({ followProdDepsOnly }) => Boolean(followProdDepsOnly), chunk.selectors)
  const prodResult = prodSelectors.length > 0
    ? await graphs.filterGraph(graphs.prodProjectsGraph, opts, prodSelectors)
    : { selected: [] as ProjectRootDir[], unmatchedFilters: [] as string[] }
  const allResult = allSelectors.length > 0
    ? await graphs.filterGraph(graphs.allProjectsGraph, opts, allSelectors)
    : { selected: [] as ProjectRootDir[], unmatchedFilters: [] as string[] }
  return {
    prodSelected: prodResult.selected,
    allSelected: allResult.selected,
    unmatchedFilters: [...prodResult.unmatchedFilters, ...allResult.unmatchedFilters],
  }
}

function applyChunkResult (
  selected: Map<ProjectRootDir, SelectionOrigin>,
  exclude: boolean,
  evalResult: ChunkEvaluationResult
): void {
  if (exclude) {
    excludeSelectedDirs(selected, [...evalResult.prodSelected, ...evalResult.allSelected])
    return
  }
  includeProdDirs(selected, evalResult.prodSelected)
  includeAllDirs(selected, evalResult.allSelected)
}

function excludeSelectedDirs (selected: Map<ProjectRootDir, SelectionOrigin>, dirs: ProjectRootDir[]): void {
  for (const dir of dirs) {
    selected.delete(dir)
  }
}

function includeProdDirs (selected: Map<ProjectRootDir, SelectionOrigin>, dirs: ProjectRootDir[]): void {
  for (const dir of dirs) {
    const existing = selected.get(dir)
    if (existing != null) {
      existing.prodMatched = true
    } else {
      selected.set(dir, { prodMatched: true, prodOnly: true })
    }
  }
}

function includeAllDirs (selected: Map<ProjectRootDir, SelectionOrigin>, dirs: ProjectRootDir[]): void {
  for (const dir of dirs) {
    const existing = selected.get(dir)
    if (existing != null) {
      existing.prodOnly = false
    } else {
      selected.set(dir, { prodMatched: false, prodOnly: false })
    }
  }
}

function finalizeSelectedProjectsGraph<Pkg extends BaseProject> (
  graphs: { allProjectsGraph: ProjectGraph<Pkg>, prodProjectsGraph?: ProjectGraph<Pkg> },
  selected: Map<ProjectRootDir, SelectionOrigin>,
  unmatchedFilters: string[]
): FilteredGraphWithProdResult<Pkg> {
  const { allProjectsGraph, prodProjectsGraph } = graphs
  const validEntries = Array.from(selected.entries()).filter(([dir]) => allProjectsGraph[dir] != null)
  const orderedEntries = [
    ...validEntries.filter(([, origin]) => origin.prodMatched),
    ...validEntries.filter(([, origin]) => !origin.prodMatched),
  ]
  const selectedProjectsGraph: ProjectGraph<Pkg> = {}
  const prodOnlySelectedProjectDirs: ProjectRootDir[] = []
  for (const [dir, origin] of orderedEntries) {
    const useProdNode = prodProjectsGraph != null && origin.prodOnly
    selectedProjectsGraph[dir] = useProdNode ? prodProjectsGraph[dir] : allProjectsGraph[dir]
    if (useProdNode) {
      prodOnlySelectedProjectDirs.push(dir)
    }
  }
  return {
    selectedProjectsGraph,
    prodOnlySelectedProjectDirs: prodProjectsGraph != null ? prodOnlySelectedProjectDirs : undefined,
    unmatchedFilters,
  }
}
