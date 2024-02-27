import { type PackageManifest, type ProjectManifest, type ReadPackageHook } from '@pnpm/types'

export function createOptionalDependenciesRemover (toBeRemoved: string[]): ReadPackageHook {
  return (manifest) => removeOptionDependencies(manifest, toBeRemoved) as PackageManifest & ProjectManifest
}

function removeOptionDependencies<Manifest extends PackageManifest | ProjectManifest> (manifest: Manifest, toBeRemoved: string[]): Manifest {
  for (const optionalDependency in manifest.optionalDependencies) {
    if (toBeRemoved.includes(optionalDependency)) {
      delete manifest.optionalDependencies[optionalDependency]
      delete manifest.dependencies?.[optionalDependency]
    }
  }
  return manifest
}
