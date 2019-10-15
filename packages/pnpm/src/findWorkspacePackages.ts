import { WORKSPACE_MANIFEST_FILENAME } from '@pnpm/constants'
import { DependencyManifest, ImporterManifest } from '@pnpm/types'
import findPackages from 'find-packages'
import path = require('path')
import readYamlFile from 'read-yaml-file'
import { LocalPackages } from 'supi'
import packageIsInstallable from './packageIsInstallable'

interface WorkspaceDependencyPackage {
  manifest: DependencyManifest
  path: string

  writeImporterManifest (manifest: ImporterManifest, force?: boolean | undefined): Promise<void>
}

export default async (
  workspaceRoot: string,
  opts: { engineStrict?: boolean },
) => {
  const packagesManifest = await requirePackagesManifest(workspaceRoot)
  const pkgs = await findPackages(workspaceRoot, {
    ignore: [
      '**/node_modules/**',
      '**/bower_components/**',
    ],
    includeRoot: true,
    patterns: packagesManifest?.packages || undefined,
  })
  pkgs.sort((pkg1: {path: string}, pkg2: {path: string}) => pkg1.path.localeCompare(pkg2.path))
  for (const pkg of pkgs) {
    packageIsInstallable(pkg.path, pkg.manifest, opts)
  }

  // FIXME: `name` and `version` might be missing from entries in `pkgs`.
  return pkgs as WorkspaceDependencyPackage[]
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
