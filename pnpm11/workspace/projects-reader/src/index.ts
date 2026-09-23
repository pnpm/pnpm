import path from 'node:path'

import { packageIsInstallable } from '@pnpm/cli.utils'
import { logger } from '@pnpm/logger'
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
  nodeVersion?: string
  sharedWorkspaceLockfile?: boolean
  supportedArchitectures?: SupportedArchitectures
}

export async function findWorkspaceProjects (
  workspaceRoot: string,
  opts?: FindWorkspaceProjectsOpts
): Promise<Project[]> {
  const projects = await findWorkspaceProjectsNoCheck(workspaceRoot, opts)
  for (const project of projects) {
    packageIsInstallable(project.rootDir, project.manifest, {
      ...opts,
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
  const fixedIgnore = new Set(['**/node_modules/**', '**/bower_components/**'])
  const toProjectModulesDir = (modulesDir: string): string | undefined => {
    if (path.isAbsolute(modulesDir)) {
      // An absolute modulesDir is one directory, skipped when it is inside the workspace.
      const relativeToWorkspace = path.relative(workspaceRoot, modulesDir)
      if (isBelow(relativeToWorkspace)) fixedIgnore.add(`${convertPathToPattern(relativeToWorkspace)}/**`)
      return undefined
    }
    const relativeToProject = path.relative('.', modulesDir)
    return isBelow(relativeToProject) && relativeToProject !== 'node_modules' ? relativeToProject : undefined
  }
  const defaultModulesDir = opts?.modulesDir == null ? undefined : toProjectModulesDir(opts.modulesDir)
  const modulesDirsByProjectName = new Map(
    Object.entries(opts?.modulesDirsByProjectName ?? {}).map(([projectName, modulesDir]) => [projectName, toProjectModulesDir(modulesDir)])
  )
  const modulesDirOf = ({ manifest }: Project): string | undefined =>
    manifest.name != null && modulesDirsByProjectName.has(manifest.name) ? modulesDirsByProjectName.get(manifest.name) : defaultModulesDir
  const allModulesDirs = new Set([defaultModulesDir, ...modulesDirsByProjectName.values()].filter((dir) => dir != null))
  return {
    initialIgnore: [...fixedIgnore, ...Array.from(allModulesDirs, (dir) => `**/${convertPathToPattern(dir)}/**`)],
    ignoreFor: (projects) => [
      ...fixedIgnore,
      ...projects.flatMap((project) => {
        const modulesDir = modulesDirOf(project)
        if (modulesDir == null) return []
        const relativeRootDir = path.relative(workspaceRoot, project.rootDir)
        const prefix = relativeRootDir === '' ? '' : `${convertPathToPattern(relativeRootDir)}/`
        return [`${prefix}${convertPathToPattern(modulesDir)}/**`]
      }),
    ],
    modulesDirOf,
    skipsProjectModulesDirs: allModulesDirs.size > 0,
  }
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

const uselessNonRootManifestFields: Array<keyof ProjectManifest> = ['resolutions']

function checkNonRootProjectManifest ({ manifest, rootDir }: Project): void {
  const warn = printNonRootFieldWarning.bind(null, rootDir)
  for (const field of uselessNonRootManifestFields) {
    if (field in manifest) {
      warn(field)
    }
  }
}

function printNonRootFieldWarning (prefix: string, propertyPath: string): void {
  const message = propertyPath === 'resolutions'
    ? `The field "${propertyPath}" was found in ${prefix}/package.json. This will not take effect. Configure dependency overrides in pnpm-workspace.yaml using the "overrides" field instead.`
    : `The field "${propertyPath}" was found in ${prefix}/package.json. This will not take effect. You should configure "${propertyPath}" at the root of the workspace instead.`

  logger.warn({
    message,
    prefix,
  })
}
