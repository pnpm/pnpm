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

const uselessNonRootManifestFields: Array<keyof ProjectManifest> = ['resolutions']

type ProjectManifestPnpm = Required<ProjectManifest>['pnpm']
const usefulNonRootPnpmFields: Array<keyof ProjectManifestPnpm> = ['executionEnv']

function checkNonRootProjectManifest ({ manifest, rootDir }: Project): void {
  const warn = printNonRootFieldWarning.bind(null, rootDir)
  for (const field of uselessNonRootManifestFields) {
    if (field in manifest) {
      warn(field)
    }
  }
  for (const field in manifest.pnpm) {
    if (!usefulNonRootPnpmFields.includes(field as keyof ProjectManifestPnpm)) {
      warn(`pnpm.${field}`)
    }
  }
}

function printNonRootFieldWarning (prefix: string, propertyPath: string): void {
  logger.warn({
    message: `The field "${propertyPath}" was found in ${prefix}/package.json. This will not take effect. You should configure "${propertyPath}" at the root of the workspace instead.`,
    prefix,
  })
}
