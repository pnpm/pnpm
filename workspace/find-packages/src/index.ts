import { packageIsInstallable } from '@pnpm/cli-utils'
import { type ProjectManifest, type Project, type SupportedArchitectures } from '@pnpm/types'
import { lexCompare } from '@pnpm/util.lex-comparator'
import { findPackages } from '@pnpm/fs.find-packages'
import { logger } from '@pnpm/logger'

export type { Project }

export type WorkspacePackagesPatterns = 'all-packages' | string[]

export interface FindWorkspacePackagesOpts {
  /**
   * An array of globs for the packages included in the workspace.
   *
   * In most cases, callers should read the pnpm-workspace.yml and pass the
   * "packages" field.
   */
  patterns?: string[]

  engineStrict?: boolean
  packageManagerStrict?: boolean
  packageManagerStrictVersion?: boolean
  nodeVersion?: string
  sharedWorkspaceLockfile?: boolean
  supportedArchitectures?: SupportedArchitectures
}

export async function findWorkspacePackages (
  workspaceRoot: string,
  opts?: FindWorkspacePackagesOpts
): Promise<Project[]> {
  const pkgs = await findWorkspacePackagesNoCheck(workspaceRoot, opts)
  for (const pkg of pkgs) {
    packageIsInstallable(pkg.rootDir, pkg.manifest, {
      ...opts,
      supportedArchitectures: opts?.supportedArchitectures ?? {
        os: ['current'],
        cpu: ['current'],
        libc: ['current'],
      },
    })
    // When setting shared-workspace-lockfile=false, `pnpm` can be set in sub-project's package.json.
    if (opts?.sharedWorkspaceLockfile && pkg.rootDir !== workspaceRoot) {
      checkNonRootProjectManifest(pkg)
    }
  }

  return pkgs
}

export async function findWorkspacePackagesNoCheck (workspaceRoot: string, opts?: { patterns?: string[] }): Promise<Project[]> {
  const pkgs = await findPackages(workspaceRoot, {
    ignore: [
      '**/node_modules/**',
      '**/bower_components/**',
    ],
    includeRoot: true,
    patterns: opts?.patterns,
  })
  pkgs.sort((pkg1: { rootDir: string }, pkg2: { rootDir: string }) => lexCompare(pkg1.rootDir, pkg2.rootDir))
  return pkgs
}

function checkNonRootProjectManifest ({ manifest, rootDir }: Project): void {
  for (const rootOnlyField of ['pnpm', 'resolutions']) {
    if (manifest?.[rootOnlyField as keyof ProjectManifest]) {
      logger.warn({
        message: `The field "${rootOnlyField}" was found in ${rootDir}/package.json. This will not take effect. You should configure "${rootOnlyField}" at the root of the workspace instead.`,
        prefix: rootDir,
      })
    }
  }
}
