import { WORKSPACE_MANIFEST_FILENAME } from '@pnpm/constants'
import { DependencyManifest } from '@pnpm/types'
import findPackages from 'find-packages'
import path = require('path')
import readYamlFile from 'read-yaml-file'

export default async (workspaceRoot: string): Promise<Array<{path: string, manifest: DependencyManifest}>> => {
  const packagesManifest = await requirePackagesManifest(workspaceRoot)
  const pkgs = await findPackages(workspaceRoot, {
    ignore: [
      '**/node_modules/**',
      '**/bower_components/**',
    ],
    patterns: packagesManifest && packagesManifest.packages || undefined,
  })
  pkgs.sort((pkg1: {path: string}, pkg2: {path: string}) => pkg1.path.localeCompare(pkg2.path))

  return pkgs
}

async function requirePackagesManifest (dir: string): Promise<{packages: string[]} | null> {
  try {
    return await readYamlFile<{ packages: string[] }>(path.join(dir, WORKSPACE_MANIFEST_FILENAME))
  } catch (err) {
    if (err['code'] === 'ENOENT') { // tslint:disable-line
      return null
    }
    throw err
  }
}

export function arrayOfLocalPackagesToMap (
  pkgs: Array<{path: string, manifest: DependencyManifest}>,
) {
  return pkgs.reduce((acc, pkg) => {
    if (!acc[pkg.manifest.name]) {
      acc[pkg.manifest.name] = {}
    }
    acc[pkg.manifest.name][pkg.manifest.version] = {
      directory: pkg.path,
      package: pkg.manifest,
    }
    return acc
  }, {})
}
