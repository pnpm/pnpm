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
  const projects = await findPackages(workspaceRoot, {
    ignore: discoveryIgnorePatterns(workspaceRoot, opts),
    includeRoot: true,
    patterns: opts?.patterns,
  })
  projects.sort((project1: { rootDir: string }, project2: { rootDir: string }) => lexCompare(project1.rootDir, project2.rootDir))
  return projects
}

export function findWorkspaceProjectsNoCheckSync (workspaceRoot: string, opts?: FindWorkspaceProjectsNoCheckOpts): Project[] {
  const projects = findPackagesSync(workspaceRoot, {
    ignore: discoveryIgnorePatterns(workspaceRoot, opts),
    includeRoot: true,
    patterns: opts?.patterns,
  })
  projects.sort((project1: { rootDir: string }, project2: { rootDir: string }) => lexCompare(project1.rootDir, project2.rootDir))
  return projects
}

function discoveryIgnorePatterns (workspaceRoot: string, opts: FindWorkspaceProjectsNoCheckOpts | undefined): string[] {
  const ignore = new Set(['**/node_modules/**', '**/bower_components/**'])
  for (const modulesDir of [opts?.modulesDir, ...(opts?.projectModulesDirs ?? [])]) {
    if (modulesDir == null) continue
    const pattern = path.isAbsolute(modulesDir)
      ? workspaceDirPattern(path.relative(workspaceRoot, modulesDir))
      : projectDirPattern(path.relative('.', modulesDir))
    if (pattern != null) ignore.add(`${pattern}/**`)
  }
  return Array.from(ignore)
}

/**
 * An absolute `modulesDir` is one directory, which only needs skipping when
 * it is inside the workspace.
 */
function workspaceDirPattern (relativeToWorkspace: string): string | undefined {
  return isBelow(relativeToWorkspace) ? convertPathToPattern(relativeToWorkspace) : undefined
}

/**
 * A relative `modulesDir` is created inside every project that uses it.
 */
function projectDirPattern (relativeToProject: string): string | undefined {
  return isBelow(relativeToProject) ? `**/${convertPathToPattern(relativeToProject)}` : undefined
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
