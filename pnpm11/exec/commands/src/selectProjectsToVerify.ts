import type { Config, ConfigContext } from '@pnpm/config.reader'
import type { ProjectRootDir, ProjectsGraph } from '@pnpm/types'
import { filterWorkspaceProjects, parseProjectSelector } from '@pnpm/workspace.projects-filter'

export type ProjectsToVerifyOptions = Partial<Pick<Config,
| 'changedFilesIgnorePattern'
| 'dir'
| 'filter'
| 'filterProd'
| 'legacyDirFiltering'
| 'testPattern'
| 'workspaceDir'
>> & Pick<ConfigContext,
| 'allProjectsGraph'
| 'prodAllProjectsGraph'
| 'prodOnlySelectedProjectDirs'
| 'selectedProjectsGraph'
>

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
  const prodOnlySelectedProjectDirs = new Set(opts.prodOnlySelectedProjectDirs)
  const selectedDirs = Object.keys(selectedProjectsGraph) as ProjectRootDir[]
  const prodAllProjectsGraph = opts.prodAllProjectsGraph
  const projects: ProjectsGraph = {
    ...selectedProjectsGraph,
    ...walkDependencies(allProjectsGraph, selectedDirs.filter((dir) => prodAllProjectsGraph == null || !prodOnlySelectedProjectDirs.has(dir))),
    ...(prodAllProjectsGraph && walkDependencies(prodAllProjectsGraph, selectedDirs.filter((dir) => prodOnlySelectedProjectDirs.has(dir)))),
  }
  if (!hasNegatedSelector(opts)) return projects
  const installedDirs = await selectInstalledProjectDirs({ ...opts, allProjectsGraph })
  return Object.fromEntries(Object.entries(projects).filter(([dir]) => installedDirs.has(dir) || selectedProjectsGraph[dir as ProjectRootDir] != null))
}

/**
 * `roots` and every project reachable from them along the edges of
 * `projectsGraph`, walking each project once however many roots reach it.
 */
function walkDependencies (projectsGraph: ProjectsGraph, roots: ProjectRootDir[]): ProjectsGraph {
  const reached: ProjectsGraph = {}
  const stack = [...roots]
  while (stack.length > 0) {
    const dir = stack.pop()!
    const node = projectsGraph[dir]
    if (reached[dir] != null || node == null) continue
    reached[dir] = node
    stack.push(...node.dependencies)
  }
  return reached
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
  const prefix = opts.dir ?? process.cwd()
  const walkOpts = {
    prefix,
    workspaceDir: opts.workspaceDir ?? prefix,
    testPattern: opts.testPattern,
    changedFilesIgnorePattern: opts.changedFilesIgnorePattern,
    useGlobDirFiltering: !opts.legacyDirFiltering,
  }
  const [regular, prod] = await Promise.all([
    filterWithDependencies(opts.allProjectsGraph, opts.filter, { ...walkOpts, followProdDepsOnly: false }),
    filterWithDependencies(opts.prodAllProjectsGraph, opts.filterProd, { ...walkOpts, followProdDepsOnly: true }),
  ])
  return new Set([...regular, ...prod])
}

async function filterWithDependencies (
  projectsGraph: ProjectsGraph | undefined,
  filter: string[] | undefined,
  opts: Parameters<typeof filterWorkspaceProjects>[2] & { followProdDepsOnly: boolean, prefix: string }
): Promise<string[]> {
  if (projectsGraph == null || filter == null || filter.length === 0) return []
  const selectors = filter.map((selector) => ({
    ...parseProjectSelector(withDependencies(selector), opts.prefix),
    followProdDepsOnly: opts.followProdDepsOnly,
  }))
  const { selectedProjectsGraph } = await filterWorkspaceProjects(projectsGraph, selectors, opts)
  return Object.keys(selectedProjectsGraph)
}

/**
 * The selector the install the gate spawns receives in place of `selector`:
 * it also selects the dependencies of what `selector` selects, because a
 * selected project needs the workspace projects it depends on installed too.
 */
export function withDependencies (selector: string): string {
  return selector.startsWith('!') || selector.endsWith('...') ? selector : `${selector}...`
}
