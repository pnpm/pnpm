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

const uselessNonRootManifestFields = {
  resolutions: 'Use the "overrides" field in pnpm-workspace.yaml at the root of the workspace instead.',
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
