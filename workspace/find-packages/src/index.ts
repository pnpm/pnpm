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
  const rootPkg = pkgs.find(pkg => pkg.rootDir === workspaceRoot)
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
    if (opts?.sharedWorkspaceLockfile && pkg !== rootPkg) {
      checkNonRootProjectManifest(pkg, rootPkg)
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

function skipWarning (pkg: Project, field: string, rootPkg?: Project | undefined): boolean {
  // pnpm.resolutions is not a thing and should be skipped.
  if (field === 'pnpm.resolutions') {
    return true
  }
  if (pkg.manifest.private) {
    return false
  }
  let overrides: Record<string, string> | undefined
  if (field === 'resolutions') {
    overrides = pkg.manifest.resolutions
  } else if (field === 'pnpm.overrides') {
    overrides = pkg.manifest.pnpm?.overrides
  }
  if (!overrides) {
    return false
  }
  // Skip warning if the public workspace package "resolutions" or "pnpm.overrides"
  // field contains overrides that entirely exist in root workspace package
  // "pnpm.overrides" field. This can happen in cases where the public workspace
  // package is to be published and consumed by other package managers.
  const rootOverrides = rootPkg?.manifest?.pnpm?.overrides
  return !!(rootOverrides && Object.entries(overrides).every(p => rootOverrides[p[0]] === p[1]))
}

function checkNonRootProjectManifest (pkg: Project, rootPkg?: Project | undefined): void {
  const warn = printNonRootFieldWarning.bind(null, pkg.rootDir)
  for (const field of uselessNonRootManifestFields) {
    if (field in pkg.manifest) {
      if (!skipWarning(pkg, field, rootPkg)) {
        warn(field)
      }
    }
  }
  for (const field in pkg.manifest.pnpm) {
    if (!usefulNonRootPnpmFields.includes(field as keyof ProjectManifestPnpm)) {
      if (!skipWarning(pkg, `pnpm.${field}`, rootPkg)) {
        warn(`pnpm.${field}`)
      }
    }
  }
}

function printNonRootFieldWarning (prefix: string, propertyPath: string): void {
  logger.warn({
    message: `The field "${propertyPath}" was found in ${prefix}/package.json. This will not take effect. You should configure "${propertyPath}" at the root of the workspace instead.`,
    prefix,
  })
}
