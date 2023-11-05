import path from 'path'
import { packageIsInstallable } from '@pnpm/cli-utils'
import { WORKSPACE_MANIFEST_FILENAME } from '@pnpm/constants'
import { type ProjectManifest, type Project, type SupportedArchitectures } from '@pnpm/types'
import { lexCompare } from '@pnpm/util.lex-comparator'
import { findPackages } from '@pnpm/fs.find-packages'
import { logger } from '@pnpm/logger'
import readYamlFile from 'read-yaml-file'
import { PnpmError } from '@pnpm/error'

export type { Project }

export async function findWorkspacePackages (
  workspaceRoot: string,
  opts?: {
    engineStrict?: boolean
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

interface WorkspaceManifest {
  packages?: string[]
}

export async function findWorkspacePackagesNoCheck (workspaceRoot: string, opts?: { patterns?: string[] }): Promise<Project[]> {
  let patterns = opts?.patterns
  if (patterns == null) {
    const packagesManifest = await requirePackagesManifest(workspaceRoot)
    validateWorkspaceManifest(packagesManifest)
    patterns = packagesManifest?.packages ?? undefined
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

async function requirePackagesManifest (dir: string): Promise<WorkspaceManifest | undefined> {
  try {
    return await readYamlFile<WorkspaceManifest>(path.join(dir, WORKSPACE_MANIFEST_FILENAME))
  } catch (err: any) { // eslint-disable-line
    if (err['code'] === 'ENOENT') {
      return undefined
    }

    throw new PnpmError('INVALID_WORKSPACE_CONFIGURATION', `\n${err.message}`)
  }
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function validateWorkspaceManifest (manifest: any): manifest is Required<WorkspaceManifest> | undefined {
  const ERR_CODE = 'INVALID_WORKSPACE_CONFIGURATION'

  if (manifest === undefined) {
    // Empty manifest is ok
    return true
  }

  if (manifest === null) {
    throw new PnpmError(ERR_CODE, 'Expected object but found - null')
  }

  if (typeof manifest !== 'object') {
    throw new PnpmError(ERR_CODE, `Expected object but found - ${typeof manifest}`)
  }

  if (Array.isArray(manifest)) {
    throw new PnpmError(ERR_CODE, 'Expected object but found - array')
  }

  if (Object.keys(manifest).length === 0) {
    // {} manifest is ok
    return true
  }

  if (!manifest.packages) {
    throw new PnpmError(ERR_CODE, 'packages field missing or empty')
  }

  if (!Array.isArray(manifest.packages)) {
    throw new PnpmError(ERR_CODE, 'packages field is not an array')
  }

  manifest.packages.forEach((pkg: unknown) => {
    if (!pkg) {
      throw new PnpmError(ERR_CODE, 'Missing or empty package')
    }

    const type = typeof pkg
    if (type !== 'string') {
      throw new PnpmError(ERR_CODE, `Invalid package type - ${type}`)
    }
  })

  return true
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

function checkNonRootProjectManifest ({ manifest, dir }: Project) {
  for (const rootOnlyField of ['pnpm', 'resolutions']) {
    if (manifest?.[rootOnlyField as keyof ProjectManifest]) {
      logger.warn({
        message: `The field "${rootOnlyField}" was found in ${dir}/package.json. This will not take effect. You should configure "${rootOnlyField}" at the root of the workspace instead.`,
        prefix: dir,
      })
    }
  }
}
