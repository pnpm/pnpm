import path from 'node:path'

import { readGlobalGroupDependencies } from '@pnpm/global.packages'
import { readPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import type { DependencyManifest } from '@pnpm/types'

export interface InstalledGroupPackage {
  alias: string
  manifest: DependencyManifest
  location: string
}

export async function readInstalledPackages (installDir: string): Promise<InstalledGroupPackage[]> {
  const depNames = Object.keys(readGlobalGroupDependencies(installDir))
  const manifests = await Promise.all(
    depNames.map((depName) => readPackageJsonFromDir(path.join(installDir, 'node_modules', depName)))
  )
  return depNames.map((depName, i) => ({
    alias: depName,
    manifest: manifests[i] as DependencyManifest,
    location: path.join(installDir, 'node_modules', depName),
  }))
}
