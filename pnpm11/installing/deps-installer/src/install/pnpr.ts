import { redactUrlForDisplay } from '@pnpm/error'
import { POST_UNINSTALL_STAGES, PRE_UNINSTALL_STAGES } from '@pnpm/exec.lifecycle'
import type { RangeSpecStyle } from '@pnpm/installing.deps-resolver'
import type { ProjectSnapshot } from '@pnpm/lockfile.fs'
import { globalWarn } from '@pnpm/logger'
import { createVersionSpecFromResolvedVersion } from '@pnpm/pkg-manifest.utils'
import { parseWantedDependency } from '@pnpm/resolving.parse-wanted-dependency'
import type { ProjectManifest, ProjectRootDir } from '@pnpm/types'
import { clone } from 'ramda'

import { removeDeps } from '../uninstall/removeDeps.js'
import type { InstallOptions, ProcessedInstallOptions as StrictInstallOptions } from './extendInstallOptions.js'
import { removesAnyDependency, rootProjectRunsPreinstallEarly } from './installPredicates.js'
import { installViaPnprServer } from './installViaPnprServer.js'
import { mergeInstallSelectors } from './mergeInstallSelectors.js'
import type {
  InstallDepsMutation,
  InstallSomeDepsMutation,
  MutatedProject,
  MutateModulesOptions,
  MutateModulesResult,
  SingleProjectInstallOptions,
} from './mutationTypes.js'

export interface PnprNewDep {
  alias: string
  /**
   * Whether the user specified a spec (e.g. `pnpm add foo@^2`). If true, the
   * manifest already has the right value and we must preserve it. If false
   * we merged in `'latest'` and need to compute a save-prefix spec from the
   * resolved version in the lockfile after the pnpr server runs.
   */
  userSpecified: boolean
}

interface PnprInstallProject {
  rootDir: ProjectRootDir
  /** The (possibly pre-processed) manifest we send to the pnpr server. */
  manifest: ProjectManifest
  mutation: MutatedProject['mutation']
  /** Newly added deps from an `installSome` mutation. Empty otherwise. */
  newDeps: PnprNewDep[]
  /** Save-prefix config for `installSome`; applied to deps whose spec defaulted to `'latest'`. */
  rangeSpecStyle?: RangeSpecStyle
}

interface PnprTarget {
  rootDir: ProjectRootDir
  manifest: ProjectManifest
  mutation?: MutatedProject
}

type PnprMutationOptions = Pick<MutateModulesOptions, 'allProjects' | 'depth' | 'ignoreScripts' | 'includeDirect'>

/**
 * Whether the pnpr server path can handle this batch of mutations. The pnpr server flow
 * supports installing the manifest as-is (`install`), adding new deps
 * (`installSome`), removing deps (`uninstallSome`), and refreshing registry
 * revisions for a complete project set. Other client-side update behavior
 * (`update`/`updateMatching`/`updateToLatest`) still uses the local resolver.
 */
export function canUsePnprForMutations (
  projects: MutatedProject[],
  opts: PnprMutationOptions
): boolean {
  if (projects.length === 0) return false
  if (!opts.ignoreScripts && removesDepsOfProjectWithUninstallScripts(projects, opts.allProjects)) {
    return false
  }
  const refreshesRevisions = projects.some(project =>
    (project.mutation === 'install' || project.mutation === 'installSome') && project.updatePatches === true
  )
  if (refreshesRevisions) {
    return canRefreshRevisionsViaPnpr(projects, opts)
  }
  return projects.every(isResolvedAsDeclared)
}

function removesDepsOfProjectWithUninstallScripts (
  projects: MutatedProject[],
  allProjects: PnprMutationOptions['allProjects']
): boolean {
  if (!projects.some((project) => project.mutation === 'uninstallSome')) return false
  const manifestsByRootDir = new Map(allProjects?.map((project) => [project.rootDir, project.manifest]))
  return projects.some((project) => {
    if (project.mutation !== 'uninstallSome') return false
    const manifest = manifestsByRootDir.get(project.rootDir)
    return removesAnyDependency(project, manifest) && definesUninstallStage(manifest?.scripts)
  })
}

function canRefreshRevisionsViaPnpr (projects: MutatedProject[], opts: PnprMutationOptions): boolean {
  if (projects.some(project =>
    project.mutation !== 'uninstallSome' &&
    !canUsePnprForPatchRefresh(opts, project.update)
  )) return false
  const { allProjects } = opts
  if (allProjects == null || projects.length !== allProjects.length) return false
  const mutatedRootDirs = new Set(projects.map(project => project.rootDir))
  if (!allProjects.every(project => mutatedRootDirs.has(project.rootDir))) return false
  return projects.every(project =>
    project.mutation === 'install' &&
    project.updatePatches === true &&
    !project.updateToLatest &&
    project.updateMatching == null
  )
}

function isResolvedAsDeclared (project: MutatedProject): boolean {
  if (project.mutation === 'uninstallSome') return true
  if (project.mutation !== 'install' && project.mutation !== 'installSome') return false
  const mutation = project as InstallDepsMutation | InstallSomeDepsMutation
  return !mutation.update && !mutation.updateToLatest && mutation.updateMatching == null
}

function definesUninstallStage (scripts: ProjectManifest['scripts']): boolean {
  return scripts != null && [...PRE_UNINSTALL_STAGES, ...POST_UNINSTALL_STAGES].some((stage) => scripts[stage] != null)
}

/**
 * Whether the configured pnpr server may resolve this install. The server runs
 * no pnpmfile, so this returns `false` and warns when the pnpmfile defines a
 * hook that shapes resolution, and the install then resolves locally
 * (https://github.com/pnpm/pnpm/issues/14460).
 */
export function pnprCanRunPnpmfile (opts: Pick<StrictInstallOptions, 'hooks' | 'pnprServer'>): boolean {
  const unsupported = pnpmfileHookPnprCannotRun(opts.hooks)
  if (unsupported == null) return true
  globalWarn(`Resolving dependencies locally because the pnpr server at ${redactUrlForDisplay(opts.pnprServer!)} cannot run the pnpmfile's ${unsupported}`)
  return false
}

export function pnpmfileHookPnprCannotRun (hooks: SingleProjectInstallOptions['hooks']): string | undefined {
  if (definesHooks(hooks?.readPackage)) return '"readPackage" hook'
  if (definesHooks(hooks?.afterAllResolved)) return '"afterAllResolved" hook'
  if (definesHooks(hooks?.preResolution)) return '"preResolution" hook'
  if (hooks?.customResolvers?.length) return 'custom resolvers'
  return undefined
}

function definesHooks (hooks: unknown[] | unknown | undefined): boolean {
  return Array.isArray(hooks) ? hooks.length > 0 : hooks != null
}

export function canUsePnprForInstall (opts: SingleProjectInstallOptions): boolean {
  if (opts.updatePatches) {
    return !opts.updateToLatest &&
      opts.updateMatching == null &&
      canUsePnprForPatchRefresh(opts, opts.update)
  }
  return !opts.update && !opts.updateToLatest && opts.updateMatching == null
}

function canUsePnprForPatchRefresh (
  opts: Pick<InstallOptions, 'depth' | 'includeDirect'>,
  update: boolean | undefined
): boolean {
  if (update !== true) return true
  return opts.depth === Infinity &&
    opts.includeDirect?.dependencies !== false &&
    opts.includeDirect?.devDependencies !== false &&
    opts.includeDirect?.optionalDependencies !== false
}

/**
 * Pre-process projects for the pnpr server flow:
 * - `install`: send the manifest as-is.
 * - `uninstallSome`: drop the named deps from the manifest before sending,
 *   so the pnpr server's resolution naturally produces a lockfile without them.
 * - `installSome`: parse selectors and merge them into the manifest.
 *
 * Returns null if the projects don't map cleanly to allProjects (caller
 * should fall through to the normal flow).
 */
async function preparePnprProjects (
  projects: MutatedProject[],
  opts: MutateModulesOptions
): Promise<PnprInstallProject[] | null> {
  const targetSet = listPnprTargets(projects, opts.allProjects ?? [])
  // Bail to the normal flow if any mutated project isn't in allProjects —
  // we can't pre-process its manifest correctly.
  for (const project of projects) {
    if (!targetSet.some((target) => target.rootDir === project.rootDir)) return null
  }
  return Promise.all(targetSet.map(async (target) => preparePnprProject(target)))
}

function listPnprTargets (
  projects: MutatedProject[],
  allProjects: NonNullable<MutateModulesOptions['allProjects']>
): PnprTarget[] {
  const mutationByRootDir = new Map<ProjectRootDir, MutatedProject>()
  for (const project of projects) {
    mutationByRootDir.set(project.rootDir, project)
  }
  // Include every workspace project, not just the mutated ones — otherwise
  // the pnpr server's resulting lockfile would only contain the targeted importer
  // and `headlessInstall` (or a later install) would crash on the missing
  // entries for the other workspace projects. Projects without a mutation
  // are sent with their current manifest (no-op for resolution).
  if (allProjects.length > 0) {
    return allProjects.map((ap) => ({
      rootDir: ap.rootDir,
      manifest: ap.manifest,
      mutation: mutationByRootDir.get(ap.rootDir),
    }))
  }
  return projects.map((project) => {
    const proj = allProjects.find((ap) => ap.rootDir === project.rootDir)
    return {
      rootDir: project.rootDir,
      manifest: proj?.manifest ?? ({} as ProjectManifest),
      mutation: project,
    }
  })
}

async function preparePnprProject (target: PnprTarget): Promise<PnprInstallProject> {
  const manifest: ProjectManifest = clone(target.manifest)
  const mutation = target.mutation
  if (mutation?.mutation === 'uninstallSome') {
    return {
      rootDir: target.rootDir,
      manifest: await removeDeps(manifest, mutation.dependencyNames, {
        prefix: mutation.rootDir,
        saveType: mutation.targetDependenciesField,
      }),
      mutation: mutation.mutation,
      newDeps: [],
      rangeSpecStyle: undefined,
    }
  }
  if (mutation?.mutation === 'installSome') {
    return {
      rootDir: target.rootDir,
      manifest: mergeInstallSelectors(manifest, mutation),
      mutation: mutation.mutation,
      newDeps: listNewDeps(mutation.dependencySelectors),
      rangeSpecStyle: mutation.rangeSpecStyle,
    }
  }
  return {
    rootDir: target.rootDir,
    manifest,
    mutation: mutation?.mutation ?? 'install',
    newDeps: [],
    rangeSpecStyle: undefined,
  }
}

function listNewDeps (dependencySelectors: string[]): PnprNewDep[] {
  const newDeps: PnprNewDep[] = []
  for (const sel of dependencySelectors) {
    const parsed = parseWantedDependency(sel)
    if (parsed.alias) {
      newDeps.push({ alias: parsed.alias, userSpecified: parsed.bareSpecifier != null })
    }
  }
  return newDeps
}

/**
 * After the pnpr server resolves, copy the lockfile importer's per-dep specifier
 * (which the server's resolver computed with the right save-prefix) back
 * into the client manifest for any newly added aliases. We rely on the
 * lockfile because the pnpr server applies catalog substitution,
 * normalizedBareSpecifier, and save-prefix logic during resolution.
 */
export function applyResolvedSpecsFromLockfile (
  manifest: ProjectManifest,
  importerSnapshot: ProjectSnapshot | undefined,
  newDeps: PnprNewDep[],
  rangeSpecStyle?: RangeSpecStyle
): ProjectManifest {
  if (!importerSnapshot || newDeps.length === 0) return manifest
  // In-memory ProjectSnapshot stores resolved versions in `dependencies`
  // (alias → resolved version) and original specs in `specifiers` (alias →
  // user spec). The on-disk YAML shape pairs them per entry — the reader
  // splits them. Read both and compute the save-prefix spec client-side.
  for (const dep of newDeps) {
    if (dep.userSpecified) continue
    applyResolvedSpec(manifest, { alias: dep.alias, importerSnapshot, rangeSpecStyle })
  }
  return manifest
}

function applyResolvedSpec (
  manifest: ProjectManifest,
  { alias, importerSnapshot, rangeSpecStyle }: { alias: string, importerSnapshot: ProjectSnapshot, rangeSpecStyle?: RangeSpecStyle }
): void {
  for (const field of ['dependencies', 'devDependencies', 'optionalDependencies'] as const) {
    const resolvedVersion = importerSnapshot[field]?.[alias]
    if (!resolvedVersion || manifest[field]?.[alias] == null) continue
    // The pnpr server resolved the tree but, on the plain-install path, it
    // writes the user's raw spec (`'latest'`) into the lockfile specifier
    // rather than normalizing to a save-prefix range. Compute the
    // save-prefix spec client-side from the resolved version.
    const savePrefixSpec = createVersionSpecFromResolvedVersion(resolvedVersion, rangeSpecStyle)
    manifest[field]![alias] = savePrefixSpec ?? resolvedVersion
  }
}

/**
 * Drives the pnpr server path for a `mutateModules` call across one or more
 * projects. Returns null if the call can't be served by the pnpr server (e.g. one
 * of the projects isn't in `allProjects`).
 */
export async function mutateModulesViaPnpr (
  projects: MutatedProject[],
  opts: MutateModulesOptions
): Promise<MutateModulesResult | null> {
  const pnprProjects = await preparePnprProjects(projects, opts)
  if (!pnprProjects) return null

  const projectOptionsByDir = new Map(opts.allProjects?.map(project => [project.rootDir, project]))

  const allInstallProjects = pnprProjects.map((pnprProject) => ({
    ...projectOptionsByDir.get(pnprProject.rootDir),
    rootDir: pnprProject.rootDir,
    manifest: pnprProject.manifest,
    mutation: pnprProject.mutation,
    newDeps: pnprProject.newDeps,
    rangeSpecStyle: pnprProject.rangeSpecStyle,
  }))

  // installViaPnprServer runs the headless install for the first
  // project's root and the workspace path for the rest. Pass the
  // pre-processed manifests so resolution sees the post-mutation state.
  const result = await installViaPnprServer({
    manifest: pnprProjects[0].manifest,
    rootDir: pnprProjects[0].rootDir,
    opts: {
      ...opts,
      updatePatches: projects.every(project =>
        project.mutation === 'install' && project.updatePatches === true
      ),
    },
    allInstallProjects,
    rootProjectPreinstallRan: rootProjectRunsPreinstallEarly(
      projects,
      { ...opts, lockfileDir: opts.lockfileDir ?? projects[0].rootDir }
    ),
  })

  const mutatedRootDirs = new Set(projects.map((project) => project.rootDir))
  const updatedProjects = allInstallProjects
    .filter((project) => mutatedRootDirs.has(project.rootDir))
    .map((project) => ({ rootDir: project.rootDir, manifest: project.manifest }))

  return {
    updatedProjects,
    stats: result.stats,
    ignoredBuilds: result.ignoredBuilds,
    resolutionPolicyViolations: result.resolutionPolicyViolations,
  }
}
