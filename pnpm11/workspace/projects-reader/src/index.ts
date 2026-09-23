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

type FindWorkspaceProjectsNoCheckOpts = Pick<FindWorkspaceProjectsOpts, 'patterns' | 'modulesDir'>

export async function findWorkspaceProjectsNoCheck (workspaceRoot: string, opts?: FindWorkspaceProjectsNoCheckOpts): Promise<Project[]> {
  const projects = await findPackages(workspaceRoot, {
    ignore: discoveryIgnorePatterns(opts?.modulesDir),
    includeRoot: true,
    patterns: opts?.patterns,
  })
  projects.sort((project1: { rootDir: string }, project2: { rootDir: string }) => lexCompare(project1.rootDir, project2.rootDir))
  return projects
}

export function findWorkspaceProjectsNoCheckSync (workspaceRoot: string, opts?: FindWorkspaceProjectsNoCheckOpts): Project[] {
  const projects = findPackagesSync(workspaceRoot, {
    ignore: discoveryIgnorePatterns(opts?.modulesDir),
    includeRoot: true,
    patterns: opts?.patterns,
  })
  projects.sort((project1: { rootDir: string }, project2: { rootDir: string }) => lexCompare(project1.rootDir, project2.rootDir))
  return projects
}

function discoveryIgnorePatterns (modulesDir: string | undefined): string[] {
  const ignore = ['**/node_modules/**', '**/bower_components/**']
  const projectModulesDir = modulesDir == null ? undefined : relativeProjectDir(modulesDir)
  if (projectModulesDir != null && projectModulesDir !== 'node_modules') {
    ignore.push(`**/${convertPathToPattern(projectModulesDir)}/**`)
  }
  return ignore
}

/**
 * `dir` as a path below a project directory, or `undefined` when it does
 * not name one: an absolute path, one that climbs out, or the project
 * directory itself.
 */
function relativeProjectDir (dir: string): string | undefined {
  if (path.isAbsolute(dir)) return undefined
  const normalized = path.relative('.', dir)
  if (normalized === '' || normalized === '..' || normalized.startsWith(`..${path.sep}`)) return undefined
  return normalized
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
