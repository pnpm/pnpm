import { packageIsInstallable } from '@pnpm/cli-utils'
import { type ProjectManifest, type Project, type SupportedArchitectures } from '@pnpm/types'
import { readWorkspaceManifest } from '@pnpm/workspace.read-manifest'
import { lexCompare } from '@pnpm/util.lex-comparator'
import { findPackages } from '@pnpm/fs.find-packages'
import { logger } from '@pnpm/logger'

export type { Project }

export async function findWorkspacePackages (
  workspaceRoot: string,
  opts?: {
    engineStrict?: boolean
    packageManagerStrict?: boolean
    nodeVersion?: string
    patterns?: string[]
    sharedWorkspaceLockfile?: boolean
    supportedArchitectures?: SupportedArchitectures
  }
): Promise<Project[]> {
  const pkgs = await findWorkspacePackagesNoCheck(workspaceRoot, opts)
  for (const pkg of pkgs) {
    packageIsInstallable(pkg.dir, pkg.manifest, opts ?? {
      supportedArchitectures: {
        os: ['current'],
        cpu: ['current'],
        libc: ['current'],
      },
    })
    // When setting shared-workspace-lockfile=false, `pnpm` can be set in sub-project's package.json.
    if (opts?.sharedWorkspaceLockfile && pkg.dir !== workspaceRoot) {
      checkNonRootProjectManifest(pkg)
    }
  }

  return pkgs
}

export async function findWorkspacePackagesNoCheck (workspaceRoot: string, opts?: { patterns?: string[] }): Promise<Project[]> {
  let patterns = opts?.patterns
  if (patterns == null) {
    const workspaceManifest = await readWorkspaceManifest(workspaceRoot)
    patterns = workspaceManifest?.packages
  }
  const pkgs = await findPackages(workspaceRoot, {
    ignore: [
      '**/node_modules/**',
      '**/bower_components/**',
    ],
    includeRoot: true,
    patterns,
  })
  pkgs.sort((pkg1: { dir: string }, pkg2: { dir: string }) => lexCompare(pkg1.dir, pkg2.dir))
  return pkgs
}

type ArrayOfWorkspacePackagesToMapResult = Record<string, Record<string, Pick<Project, 'manifest'>>>

export function arrayOfWorkspacePackagesToMap (
  pkgs: Array<Pick<Project, 'manifest'>>
): ArrayOfWorkspacePackagesToMapResult {
  return pkgs.reduce((acc, pkg) => {
    if (!pkg.manifest.name) return acc
    if (!acc[pkg.manifest.name]) {
      acc[pkg.manifest.name] = {}
    }
    acc[pkg.manifest.name][pkg.manifest.version ?? '0.0.0'] = pkg
    return acc
  }, {} as ArrayOfWorkspacePackagesToMapResult)
}

function checkNonRootProjectManifest ({ manifest, dir }: Project): void {
  for (const rootOnlyField of ['pnpm', 'resolutions']) {
    if (manifest?.[rootOnlyField as keyof ProjectManifest]) {
      logger.warn({
        message: `The field "${rootOnlyField}" was found in ${dir}/package.json. This will not take effect. You should configure "${rootOnlyField}" at the root of the workspace instead.`,
        prefix: dir,
      })
    }
  }
}
