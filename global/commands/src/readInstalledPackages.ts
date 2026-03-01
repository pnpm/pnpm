import path from 'path'
import { readPackageJsonFromDir, readPackageJsonFromDirRawSync } from '@pnpm/read-package-json'
import { type DependencyManifest } from '@pnpm/types'

export async function readInstalledPackages (installDir: string): Promise<Array<{ manifest: DependencyManifest, location: string }>> {
  const pkgJson = readPackageJsonFromDirRawSync(installDir)
  const depNames = Object.keys(pkgJson.dependencies ?? {})
  const manifests = await Promise.all(
    depNames.map((depName) => readPackageJsonFromDir(path.join(installDir, 'node_modules', depName)))
  )
  return depNames.map((depName, i) => ({
    manifest: manifests[i] as DependencyManifest,
    location: path.join(installDir, 'node_modules', depName),
  }))
}
