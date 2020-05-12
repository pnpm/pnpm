import { packageIsInstallable } from '@pnpm/cli-utils'
import { Project } from '@pnpm/config'
import { WORKSPACE_MANIFEST_FILENAME } from '@pnpm/constants'
import findPackages from 'find-packages'
import path = require('path')
import readYamlFile from 'read-yaml-file'

export { Project }

export default async (
  workspaceRoot: string,
  opts?: {
    engineStrict?: boolean,
    patterns?: string[],
  }
) => {
  let patterns = opts?.patterns
  if (!patterns) {
    const packagesManifest = await requirePackagesManifest(workspaceRoot)
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
  pkgs.sort((pkg1: {dir: string}, pkg2: {dir: string}) => pkg1.dir.localeCompare(pkg2.dir))
  for (const pkg of pkgs) {
    packageIsInstallable(pkg.dir, pkg.manifest, opts ?? {})
  }

  return pkgs as Project[]
}

async function requirePackagesManifest (dir: string): Promise<{packages?: string[]} | null> {
  try {
    return await readYamlFile<{ packages?: string[] }>(path.join(dir, WORKSPACE_MANIFEST_FILENAME))
  } catch (err) {
    if (err['code'] === 'ENOENT') { // tslint:disable-line
      return null
    }
    throw err
  }
}

export function arrayOfWorkspacePackagesToMap (
  pkgs: Project[]
) {
  return pkgs.reduce((acc, pkg) => {
    if (!pkg.manifest.name || !pkg.manifest.version) return acc
    if (!acc[pkg.manifest.name]) {
      acc[pkg.manifest.name] = {}
    }
    acc[pkg.manifest.name][pkg.manifest.version] = pkg
    return acc
  }, {})
}
