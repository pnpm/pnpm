import { packageIsInstallable } from '@pnpm/cli.utils'
import { logger } from '@pnpm/logger'
import type { Project, ProjectManifest, SupportedArchitectures } from '@pnpm/types'
import { lexCompare } from '@pnpm/util.lex-comparator'

import { findPackages } from './findPackages.js'

export { findPackages, type FindPackagesOptions } from './findPackages.js'
export type { Project }

export interface FindWorkspaceProjectsOpts {
  /**
   * An array of globs for the packages included in the workspace.
   *
   * In most cases, callers should read the pnpm-workspace.yml and pass the
   * "packages" field.
   */
  patterns?: string[]

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

export async function findWorkspaceProjectsNoCheck (workspaceRoot: string, opts?: { patterns?: string[] }): Promise<Project[]> {
  const projects = await findPackages(workspaceRoot, {
    ignore: [
      '**/node_modules/**',
      '**/bower_components/**',
    ],
    includeRoot: true,
    patterns: opts?.patterns,
  })
  projects.sort((project1: { rootDir: string }, project2: { rootDir: string }) => lexCompare(project1.rootDir, project2.rootDir))
  return projects
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
  logger.warn({
    message: `The field "${propertyPath}" was found in ${prefix}/package.json. This will not take effect. You should configure "${propertyPath}" at the root of the workspace instead.`,
    prefix,
  })
}
