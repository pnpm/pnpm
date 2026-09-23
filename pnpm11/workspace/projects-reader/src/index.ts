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
   * The `modulesDir` values that `packageConfigs` sets for individual
   * projects, skipped the same way.
   */
  projectModulesDirs?: string[]

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

type FindWorkspaceProjectsNoCheckOpts = Pick<FindWorkspaceProjectsOpts, 'patterns' | 'modulesDir' | 'projectModulesDirs'>

export async function findWorkspaceProjectsNoCheck (workspaceRoot: string, opts?: FindWorkspaceProjectsNoCheckOpts): Promise<Project[]> {
  const plan = planDiscovery(workspaceRoot, opts)
  const find = async (ignore: string[]) => findPackages(workspaceRoot, { ignore, includeRoot: true, patterns: opts?.patterns })
  let projects = await find(plan.initialIgnore)
  if (plan.projectModulesDirs.length > 0) {
    for (;;) {
      // Each walk skips the modules directories of the projects the previous one found.
      // eslint-disable-next-line no-await-in-loop
      const grown = addProjectsOutsideModulesDirs(projects, await find(plan.ignoreFor(projects)), plan.projectModulesDirs)
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
  if (plan.projectModulesDirs.length > 0) {
    for (;;) {
      const grown = addProjectsOutsideModulesDirs(projects, find(plan.ignoreFor(projects)), plan.projectModulesDirs)
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
  /** Relative `modulesDir` values, created inside every project that uses them. */
  projectModulesDirs: string[]
}

function planDiscovery (workspaceRoot: string, opts: FindWorkspaceProjectsNoCheckOpts | undefined): DiscoveryPlan {
  const fixedIgnore = new Set(['**/node_modules/**', '**/bower_components/**'])
  const projectModulesDirs = new Set<string>()
  for (const modulesDir of [opts?.modulesDir, ...(opts?.projectModulesDirs ?? [])]) {
    if (modulesDir == null) continue
    if (path.isAbsolute(modulesDir)) {
      // An absolute modulesDir is one directory, skipped when it is inside the workspace.
      const relativeToWorkspace = path.relative(workspaceRoot, modulesDir)
      if (isBelow(relativeToWorkspace)) fixedIgnore.add(`${convertPathToPattern(relativeToWorkspace)}/**`)
      continue
    }
    const relativeToProject = path.relative('.', modulesDir)
    if (isBelow(relativeToProject) && relativeToProject !== 'node_modules') projectModulesDirs.add(relativeToProject)
  }
  const modulesDirPatterns = Array.from(projectModulesDirs, convertPathToPattern)
  return {
    initialIgnore: [...fixedIgnore, ...modulesDirPatterns.map((dir) => `**/${dir}/**`)],
    ignoreFor: (projects) => [
      ...fixedIgnore,
      ...projects.flatMap(({ rootDir }) => {
        const relativeRootDir = path.relative(workspaceRoot, rootDir)
        const prefix = relativeRootDir === '' ? '' : `${convertPathToPattern(relativeRootDir)}/`
        return modulesDirPatterns.map((dir) => `${prefix}${dir}/**`)
      }),
    ],
    projectModulesDirs: Array.from(projectModulesDirs),
  }
}

/**
 * `known` plus the projects in `found` that are not inside a modules
 * directory of a known or found project, or `undefined` when that adds
 * none. The result only ever grows, so repeating the walk terminates.
 */
function addProjectsOutsideModulesDirs (known: Project[], found: Project[], projectModulesDirs: string[]): Project[] | undefined {
  const knownDirs = new Set(known.map(({ rootDir }) => rootDir))
  const projectDirs = new Set([...knownDirs, ...found.map(({ rootDir }) => rootDir)])
  const added = found.filter(({ rootDir }) => !knownDirs.has(rootDir) && !isInsideModulesDirOfProject(rootDir, projectDirs, projectModulesDirs))
  return added.length === 0 ? undefined : [...known, ...added]
}

function isInsideModulesDirOfProject (dir: string, projectDirs: Set<string>, projectModulesDirs: string[]): boolean {
  for (let ancestor = path.dirname(dir); ; ancestor = path.dirname(ancestor)) {
    if (projectDirs.has(ancestor) && projectModulesDirs.some((modulesDir) => dir.startsWith(`${path.join(ancestor, modulesDir)}${path.sep}`))) {
      return true
    }
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
