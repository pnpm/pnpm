import { type PackageManifest, type ProjectManifest, type ReadPackageHook } from '@pnpm/types'

export function createOptionalDependenciesRemover (toBeRemoved: string[]): ReadPackageHook {
  return <Manifest extends PackageManifest | ProjectManifest> (manifest: Manifest) => removeOptionalDependencies(manifest, toBeRemoved)
}

function removeOptionalDependencies<Manifest extends PackageManifest | ProjectManifest> (manifest: Manifest, toBeRemoved: string[]): Manifest {
  for (const optionalDependency in manifest.optionalDependencies) {
    if (toBeRemoved.includes(optionalDependency)) {
      delete manifest.optionalDependencies[optionalDependency]
      delete manifest.dependencies?.[optionalDependency]
    }
  }
  return manifest
}
