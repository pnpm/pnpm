import path from 'node:path'

import type { Config, ConfigContext } from '@pnpm/config.reader'
import type { ProjectManifest, ProjectRootDir, ProjectsGraph } from '@pnpm/types'
import {
  filterWorkspaceProjectsWithProdGraph,
  orderWorkspaceFilters,
  parseProjectSelector,
} from '@pnpm/workspace.projects-filter'

export type ProjectsToVerifyOptions = Partial<Pick<Config,
| 'changedFilesIgnorePattern'
| 'dir'
| 'filter'
| 'filterProd'
| 'injectWorkspacePackages'
| 'legacyDirFiltering'
| 'sharedWorkspaceLockfile'
| 'testPattern'
| 'workspaceDir'
>> & Pick<ConfigContext,
| 'allProjectsGraph'
| 'prodAllProjectsGraph'
| 'prodOnlySelectedProjectDirs'
| 'selectedProjectsGraph'
> & {
  argv?: {
    cooked?: string[]
  }
  cliOptions?: Record<string, unknown>
}

/**
 * The projects the dependency status check holds to the modules-directory
 * requirement: the selected projects and the workspace projects they depend
 * on, which the install the gate spawns selects too
 * (https://github.com/pnpm/tasks/issues/45). A dependency a negated selector
 * excludes is left out, because that selector reaches the install unchanged.
 * The selected projects themselves always stay.
 *
 * A project selected only by `--filter-prod` reads its edges from the
 * prod-pruned graph, so its dev-only workspace dependencies are not required.
 * A non-recursive command has no workspace graph, so its selection is
 * returned as it is.
 */
export async function selectProjectsToVerify (opts: ProjectsToVerifyOptions): Promise<ProjectsGraph | undefined> {
  const { allProjectsGraph, selectedProjectsGraph } = opts
  if (selectedProjectsGraph == null || allProjectsGraph == null) return selectedProjectsGraph
  // An injected workspace dependency is a copy inside the project that
  // depends on it, which that project's own install covers; explicit `link:`
  // dependencies remain symlinks and still need their target project installed.
  const onlyLinked = Boolean(opts.sharedWorkspaceLockfile === false && opts.injectWorkspacePackages)
  const prodOnlySelectedProjectDirs = new Set(opts.prodOnlySelectedProjectDirs)
  const selectedDirs = Object.keys(selectedProjectsGraph) as ProjectRootDir[]
  const prodAllProjectsGraph = opts.prodAllProjectsGraph
  const projects: ProjectsGraph = {
    ...selectedProjectsGraph,
    ...walkDependencies(allProjectsGraph, selectedDirs.filter((dir) => prodAllProjectsGraph == null || !prodOnlySelectedProjectDirs.has(dir)), onlyLinked),
    ...(prodAllProjectsGraph && walkDependencies(prodAllProjectsGraph, selectedDirs.filter((dir) => prodOnlySelectedProjectDirs.has(dir)), onlyLinked)),
  }
  if (!hasNegatedSelector(opts)) return projects
  const installedDirs = await selectInstalledProjectDirs({ ...opts, allProjectsGraph })
  return Object.fromEntries(Object.entries(projects).filter(([dir]) => installedDirs.has(dir) || selectedProjectsGraph[dir as ProjectRootDir] != null))
}

/**
 * `roots` and every project reachable from them along the edges of
 * `projectsGraph`, walking each project once however many roots reach it.
 */
function walkDependencies (projectsGraph: ProjectsGraph, roots: ProjectRootDir[], onlyLinked = false): ProjectsGraph {
  const reached: ProjectsGraph = {}
  const stack = [...roots]
  while (stack.length > 0) {
    const dir = stack.pop()!
    const node = projectsGraph[dir]
    if (reached[dir] != null || node == null) continue
    reached[dir] = node
    let dependencies = node.dependencies
    if (onlyLinked && dependencies.length > 0) {
      const linkedTargets = linkedDependencyDirs(dir, node.package.manifest)
      dependencies = dependencies.filter((depDir) => linkedTargets.has(path.resolve(depDir)))
    }
    stack.push(...dependencies)
  }
  return reached
}

function linkedDependencyDirs (parentDir: ProjectRootDir, manifest: ProjectManifest | undefined): Set<string> {
  const targets = new Set<string>()
  if (manifest == null) return targets
  for (const field of ['dependencies', 'devDependencies', 'optionalDependencies', 'peerDependencies'] as const) {
    for (const spec of Object.values(manifest[field] ?? {})) {
      if (typeof spec === 'string' && spec.startsWith('link:')) {
        targets.add(path.resolve(parentDir, spec.slice('link:'.length).replace(/\\/g, '/')))
      }
    }
  }
  return targets
}

function hasNegatedSelector (opts: ProjectsToVerifyOptions): boolean {
  return [...opts.filter ?? [], ...opts.filterProd ?? []].some((selector) => selector.startsWith('!'))
}

/**
 * The projects the install the gate spawns selects, evaluated against the
 * graphs the command's own selection was drawn from. That install runs in
 * `dir`, so its path selectors resolve there.
 */
async function selectInstalledProjectDirs (
  opts: ProjectsToVerifyOptions & Required<Pick<ProjectsToVerifyOptions, 'allProjectsGraph'>>
): Promise<Set<string>> {
  const ordered = orderWorkspaceFilters({
    filter: opts.filter,
    filterProd: opts.filterProd,
    cliOptions: opts.cliOptions,
    cookedArgv: opts.argv?.cooked,
  })
  if (ordered.length === 0) return new Set()
  const prefix = opts.dir ?? process.cwd()
  const selectors = ordered.map(({ filter, followProdDepsOnly }) => ({
    ...parseProjectSelector(withDependencies(filter), prefix),
    followProdDepsOnly,
  }))
  const { selectedProjectsGraph } = await filterWorkspaceProjectsWithProdGraph(
    { allProjectsGraph: opts.allProjectsGraph, prodProjectsGraph: opts.prodAllProjectsGraph },
    selectors,
    {
      workspaceDir: opts.workspaceDir ?? prefix,
      testPattern: opts.testPattern,
      changedFilesIgnorePattern: opts.changedFilesIgnorePattern,
      useGlobDirFiltering: !opts.legacyDirFiltering,
    }
  )
  return new Set(Object.keys(selectedProjectsGraph))
}

/**
 * The selector the install the gate spawns receives in place of `selector`:
 * it also selects the dependencies of what `selector` selects, because a
 * selected project needs the workspace projects it depends on installed too.
 */
export function withDependencies (selector: string): string {
  return selector.startsWith('!') || selector.endsWith('...') ? selector : `${selector}...`
}
