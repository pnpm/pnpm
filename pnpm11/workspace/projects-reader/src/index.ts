import path from 'node:path'

import { packageIsInstallable } from '@pnpm/cli.utils'
import { isError } from '@pnpm/error'
import { readWantedLockfile } from '@pnpm/lockfile.fs'
import { findLockedNodeRuntime, getLockfileImporterId } from '@pnpm/lockfile.utils'
import { logger } from '@pnpm/logger'
import { getDownloadedNodeRuntimeSpecifierFromEnginesRuntime, getDownloadedNodeVersionFromEnginesRuntime } from '@pnpm/pkg-manifest.utils'
import { lexCompare } from '@pnpm/text.ordinal-comparator'
import type { Project, ProjectManifest, SupportedArchitectures } from '@pnpm/types'
import { convertPathToPattern } from 'tinyglobby'

import { findPackages, findPackagesSync } from './findPackages.js'

export { findPackages, type FindPackagesOptions, findPackagesSync } from './findPackages.js'
export type { Project }

export interface FindWorkspaceProjectsOpts {
  /**
   * An array of globs for the packages included in the workspace.
   *
   * In most cases, callers should read the pnpm-workspace.yml and pass the
   * "packages" field.
   */
  patterns?: string[]

  /**
   * The configured `modulesDir`. An install creates it inside every project,
   * so, like `node_modules`, it is never searched for projects.
   */
  modulesDir?: string

  /**
   * The `modulesDir` that `packageConfigs` sets for a project, keyed by
   * project name. It replaces `modulesDir` for that project.
   */
  modulesDirsByProjectName?: Record<string, string>

  engineStrict?: boolean
  lockfile?: boolean
  dev?: boolean
  lockfileDir?: string
  nodeVersion?: string
  nodeVersionFromEnginesRuntime?: boolean
  production?: boolean
  runtimeOnFail?: 'download' | 'error' | 'warn' | 'ignore'
  skipRuntimes?: boolean
  sharedWorkspaceLockfile?: boolean
  useGitBranchLockfile?: boolean
  mergeGitBranchLockfiles?: boolean
  supportedArchitectures?: SupportedArchitectures
}

export async function findWorkspaceProjects (
  workspaceRoot: string,
  opts?: FindWorkspaceProjectsOpts
): Promise<Project[]> {
  const projects = await findWorkspaceProjectsNoCheck(workspaceRoot, opts)
  const lockedNodeVersions = await readLockedNodeVersions({ workspaceRoot, projects, opts })
  for (const project of projects) {
    packageIsInstallable(project.rootDir, project.manifest, {
      ...opts,
      nodeVersion: getProjectNodeVersion({ workspaceRoot, projectRoot: project.rootDir, manifest: project.manifest, opts, lockedNodeVersions }),
      supportedArchitectures: opts?.supportedArchitectures ?? {
        os: ['current'],
        cpu: ['current'],
        libc: ['current'],
      },
    })
    // When setting shared-workspace-lockfile=false, `pnpm` can be set in sub-project's package.json.
    if (opts?.sharedWorkspaceLockfile && project.rootDir !== workspaceRoot) {
      checkNonRootProjectManifest(project)
    }
  }

  return projects
}

export function findWorkspaceProjectsSync (
  workspaceRoot: string,
  opts?: FindWorkspaceProjectsOpts
): Project[] {
  const projects = findWorkspaceProjectsNoCheckSync(workspaceRoot, opts)
  for (const project of projects) {
    packageIsInstallable(project.rootDir, project.manifest, {
      ...opts,
      nodeVersion: getProjectNodeVersion({ workspaceRoot, projectRoot: project.rootDir, manifest: project.manifest, opts }),
      supportedArchitectures: opts?.supportedArchitectures ?? {
        os: ['current'],
        cpu: ['current'],
        libc: ['current'],
      },
    })
    if (opts?.sharedWorkspaceLockfile && project.rootDir !== workspaceRoot) {
      checkNonRootProjectManifest(project)
    }
  }

  return projects
}

function getProjectNodeVersion (options: {
  workspaceRoot: string
  projectRoot: string
  manifest: ProjectManifest
  opts?: FindWorkspaceProjectsOpts
  lockedNodeVersions?: Record<string, string>
}): string | undefined {
  const { workspaceRoot, projectRoot, manifest, opts, lockedNodeVersions } = options
  if (opts?.skipRuntimes) {
    return opts.nodeVersionFromEnginesRuntime ? undefined : opts.nodeVersion
  }
  if (opts?.nodeVersion != null && !opts.nodeVersionFromEnginesRuntime) return opts.nodeVersion
  const dependencyGroup = getRuntimeDependencyGroup(opts)
  const declaredNodeVersion = getDownloadedNodeVersionFromEnginesRuntime(manifest, opts?.runtimeOnFail, dependencyGroup)
  return lockedNodeVersions?.[projectRoot] ?? declaredNodeVersion ?? lockedNodeVersions?.[workspaceRoot] ?? opts?.nodeVersion
}

async function readLockedNodeVersions (options: {
  workspaceRoot: string
  projects: Project[]
  opts?: FindWorkspaceProjectsOpts
}): Promise<Record<string, string>> {
  const { workspaceRoot, projects, opts } = options
  if (!shouldReadLockedNodeVersions(projects, opts)) return {}
  const projectsByLockfileDir = groupProjectsByLockfileDir({ workspaceRoot, projects, opts })
  const versionsByLockfile = await Promise.all(
    Array.from(projectsByLockfileDir, async ([lockfileDir, projects]) => {
      const lockfile = await readNodeRuntimeLockfile({ lockfileDir, opts })
      if (lockfile == null) return {}
      return lockedNodeVersionsFromLockfile({ lockfile, lockfileDir, projects, opts })
    })
  )
  return Object.assign({}, ...versionsByLockfile)
}

function groupProjectsByLockfileDir (options: {
  workspaceRoot: string
  projects: Project[]
  opts?: FindWorkspaceProjectsOpts
}): Map<string, Project[]> {
  const { workspaceRoot, projects, opts } = options
  const groups = new Map<string, Project[]>()
  for (const project of projects) {
    if (!hasDownloadedNodeRuntime(project.manifest, opts)) continue
    const lockfileDir = opts?.lockfileDir ?? (opts?.sharedWorkspaceLockfile === false ? project.rootDir : workspaceRoot)
    const lockfileProjects = groups.get(lockfileDir) ?? []
    lockfileProjects.push(project)
    groups.set(lockfileDir, lockfileProjects)
  }
  return groups
}

async function readNodeRuntimeLockfile (options: {
  lockfileDir: string
  opts?: FindWorkspaceProjectsOpts
}): Promise<Awaited<ReturnType<typeof readWantedLockfile>>> {
  const { lockfileDir, opts } = options
  try {
    return await readWantedLockfile(lockfileDir, {
      ignoreIncompatible: true,
      useGitBranchLockfile: opts?.useGitBranchLockfile,
      mergeGitBranchLockfiles: opts?.mergeGitBranchLockfiles,
    })
  } catch (error: unknown) {
    if (isError(error) && 'code' in error && error.code === 'ERR_PNPM_BROKEN_LOCKFILE') return null
    throw error
  }
}

function shouldReadLockedNodeVersions (projects: Project[], opts?: FindWorkspaceProjectsOpts): boolean {
  if (opts?.lockfile === false || opts?.skipRuntimes || opts?.nodeVersion != null && !opts.nodeVersionFromEnginesRuntime) return false
  if (opts?.nodeVersionFromEnginesRuntime === true) return true
  return projects.some(project => hasDownloadedNodeRuntime(project.manifest, opts))
}

function lockedNodeVersionsFromLockfile (options: {
  lockfile: NonNullable<Awaited<ReturnType<typeof readWantedLockfile>>>
  lockfileDir: string
  projects: Project[]
  opts?: FindWorkspaceProjectsOpts
}): Record<string, string> {
  const { lockfile, lockfileDir, projects, opts } = options
  const dependencyFields = getRuntimeDependencyFields(opts)
  const lockedNodeVersions: Record<string, string> = {}
  for (const project of projects) {
    if (!hasDownloadedNodeRuntime(project.manifest, opts)) continue
    const importerId = getLockfileImporterId(lockfileDir, project.rootDir)
    const runtime = findLockedNodeRuntime(lockfile, importerId, dependencyFields)
    const declaredSpecifier = getDownloadedNodeRuntimeSpecifierFromEnginesRuntime(
      project.manifest,
      opts?.runtimeOnFail,
      getRuntimeDependencyGroup(opts)
    )
    if (runtime != null && runtime.specifier === declaredSpecifier) lockedNodeVersions[project.rootDir] = runtime.version
  }
  return lockedNodeVersions
}

function hasDownloadedNodeRuntime (manifest: ProjectManifest, opts?: FindWorkspaceProjectsOpts): boolean {
  return getDownloadedNodeVersionFromEnginesRuntime(
    manifest,
    opts?.runtimeOnFail,
    getRuntimeDependencyGroup(opts)
  ) != null
}

function getRuntimeDependencyGroup (opts?: FindWorkspaceProjectsOpts): 'dependencies' | 'devDependencies' | undefined {
  return opts?.production && !opts.dev
    ? 'dependencies'
    : opts?.dev && !opts.production
      ? 'devDependencies'
      : undefined
}

function getRuntimeDependencyFields (
  opts?: FindWorkspaceProjectsOpts
): readonly ('dependencies' | 'devDependencies' | 'optionalDependencies')[] | undefined {
  return opts?.production && !opts.dev
    ? ['dependencies', 'optionalDependencies']
    : opts?.dev && !opts.production
      ? ['devDependencies']
      : undefined
}

type FindWorkspaceProjectsNoCheckOpts = Pick<FindWorkspaceProjectsOpts, 'patterns' | 'modulesDir' | 'modulesDirsByProjectName'>

export async function findWorkspaceProjectsNoCheck (workspaceRoot: string, opts?: FindWorkspaceProjectsNoCheckOpts): Promise<Project[]> {
  const plan = planDiscovery(workspaceRoot, opts)
  const find = async (ignore: string[]) => findPackages(workspaceRoot, { ignore, includeRoot: true, patterns: opts?.patterns })
  let projects = await find(plan.initialIgnore)
  if (plan.skipsProjectModulesDirs) {
    for (;;) {
      // eslint-disable-next-line no-await-in-loop -- a walk's ignores come from the previous walk
      const grown = addProjectsOutsideModulesDirs(projects, await find(plan.ignoreFor(projects)), plan.modulesDirOf)
      if (grown == null) break
      projects = grown
    }
  }
  return projects.sort(compareRootDirs)
}

export function findWorkspaceProjectsNoCheckSync (workspaceRoot: string, opts?: FindWorkspaceProjectsNoCheckOpts): Project[] {
  const plan = planDiscovery(workspaceRoot, opts)
  const find = (ignore: string[]) => findPackagesSync(workspaceRoot, { ignore, includeRoot: true, patterns: opts?.patterns })
  let projects = find(plan.initialIgnore)
  if (plan.skipsProjectModulesDirs) {
    for (;;) {
      const grown = addProjectsOutsideModulesDirs(projects, find(plan.ignoreFor(projects)), plan.modulesDirOf)
      if (grown == null) break
      projects = grown
    }
  }
  return projects.sort(compareRootDirs)
}

function compareRootDirs (project1: { rootDir: string }, project2: { rootDir: string }): number {
  return lexCompare(project1.rootDir, project2.rootDir)
}

interface DiscoveryPlan {
  /**
   * Skips every directory named like a project modules directory. A walk
   * with it finds no dependency, but may also miss a project that sits
   * in such a directory without a project owning it.
   */
  initialIgnore: string[]
  /** Skips the modules directories of the given projects only. */
  ignoreFor: (projects: Project[]) => string[]
  /** The relative modules directory created inside `project`, if any. */
  modulesDirOf: (project: Project) => string | undefined
  skipsProjectModulesDirs: boolean
}

function planDiscovery (workspaceRoot: string, opts: FindWorkspaceProjectsNoCheckOpts | undefined): DiscoveryPlan {
  const { fixedIgnore, defaultModulesDir, modulesDirsByProjectName } = resolveConfiguredModulesDirs(workspaceRoot, opts)
  const modulesDirOf = ({ manifest }: Project): string | undefined =>
    manifest.name != null && modulesDirsByProjectName.has(manifest.name) ? modulesDirsByProjectName.get(manifest.name) : defaultModulesDir
  const allModulesDirs = new Set([defaultModulesDir, ...modulesDirsByProjectName.values()].filter((dir) => dir != null))
  return {
    initialIgnore: [...fixedIgnore, ...Array.from(allModulesDirs, (dir) => `**/${convertPathToPattern(dir)}/**`)],
    ignoreFor: (projects) => buildIgnoreForProjects(workspaceRoot, projects, fixedIgnore, modulesDirOf),
    modulesDirOf,
    skipsProjectModulesDirs: allModulesDirs.size > 0,
  }
}

interface ConfiguredModulesDirs {
  fixedIgnore: Set<string>
  defaultModulesDir: string | undefined
  modulesDirsByProjectName: Map<string, string | undefined>
}

function resolveConfiguredModulesDirs (
  workspaceRoot: string,
  opts: FindWorkspaceProjectsNoCheckOpts | undefined
): ConfiguredModulesDirs {
  const fixedIgnore = new Set(['**/node_modules/**', '**/bower_components/**'])
  const toProjectModulesDir = (modulesDir: string): string | undefined =>
    normalizeProjectModulesDir(workspaceRoot, modulesDir, fixedIgnore)
  const defaultModulesDir = opts?.modulesDir == null ? undefined : toProjectModulesDir(opts.modulesDir)
  const modulesDirsByProjectName = new Map(
    Object.entries(opts?.modulesDirsByProjectName ?? {}).map(([projectName, modulesDir]) => [projectName, toProjectModulesDir(modulesDir)])
  )
  return { defaultModulesDir, fixedIgnore, modulesDirsByProjectName }
}

function normalizeProjectModulesDir (
  workspaceRoot: string,
  modulesDir: string,
  fixedIgnore: Set<string>
): string | undefined {
  if (path.isAbsolute(modulesDir)) {
    const relativeToWorkspace = path.relative(workspaceRoot, modulesDir)
    if (isBelow(relativeToWorkspace)) fixedIgnore.add(`${convertPathToPattern(relativeToWorkspace)}/**`)
    return undefined
  }
  const relativeToProject = path.relative('.', modulesDir)
  return isBelow(relativeToProject) && relativeToProject !== 'node_modules' ? relativeToProject : undefined
}

function buildIgnoreForProjects (
  workspaceRoot: string,
  projects: Project[],
  fixedIgnore: Set<string>,
  modulesDirOf: (project: Project) => string | undefined
): string[] {
  return [
    ...fixedIgnore,
    ...projects.flatMap((project) => {
      const modulesDir = modulesDirOf(project)
      if (modulesDir == null) return []
      const relativeRootDir = path.relative(workspaceRoot, project.rootDir)
      const prefix = relativeRootDir === '' ? '' : `${convertPathToPattern(relativeRootDir)}/`
      return [`${prefix}${convertPathToPattern(modulesDir)}/**`]
    }),
  ]
}

/**
 * `known` plus the projects in `found` that are not inside the modules
 * directory of a known or found project, or `undefined` when that adds
 * none. The result only ever grows, so repeating the walk terminates.
 */
function addProjectsOutsideModulesDirs (
  known: Project[],
  found: Project[],
  modulesDirOf: (project: Project) => string | undefined
): Project[] | undefined {
  const knownDirs = new Set(known.map(({ rootDir }) => rootDir))
  const ownedModulesDirs = new Map<string, string>()
  for (const project of [...known, ...found]) {
    const modulesDir = modulesDirOf(project)
    if (modulesDir != null) ownedModulesDirs.set(project.rootDir, path.join(project.rootDir, modulesDir))
  }
  const added = found.filter(({ rootDir }) => !knownDirs.has(rootDir) && !isInsideOwnedModulesDir(rootDir, ownedModulesDirs))
  return added.length === 0 ? undefined : [...known, ...added]
}

function isInsideOwnedModulesDir (dir: string, ownedModulesDirs: Map<string, string>): boolean {
  for (let ancestor = path.dirname(dir); ; ancestor = path.dirname(ancestor)) {
    const modulesDir = ownedModulesDirs.get(ancestor)
    if (modulesDir != null && dir.startsWith(`${modulesDir}${path.sep}`)) return true
    if (path.dirname(ancestor) === ancestor) return false
  }
}

function isBelow (relativePath: string): boolean {
  return relativePath !== '' && relativePath !== '..' && !relativePath.startsWith(`..${path.sep}`) && !path.isAbsolute(relativePath)
}

const uselessNonRootManifestFields = {
  resolutions: 'Configure dependency overrides in pnpm-workspace.yaml using the "overrides" field instead.',
} satisfies Partial<Record<keyof ProjectManifest, string>>

function checkNonRootProjectManifest ({ manifest, rootDir }: Project): void {
  for (const [field, suggestion] of Object.entries(uselessNonRootManifestFields)) {
    if (field in manifest) {
      logger.warn({
        message: `The field "${field}" was found in ${rootDir}/package.json. This will not take effect. ${suggestion}`,
        prefix: rootDir,
      })
    }
  }
}
