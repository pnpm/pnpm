import type { BaseManifest, ReadPackageHook } from '@pnpm/types'
import { createMatcher } from '@pnpm/matcher'

export function createOptionalDependenciesRemover (toBeRemoved: string[]): ReadPackageHook {
  if (!toBeRemoved.length) return <Manifest extends BaseManifest>(manifest: Manifest) => manifest
  const shouldBeRemoved = createMatcher(toBeRemoved)
  return <Manifest extends BaseManifest> (manifest: Manifest) => removeOptionalDependencies(manifest, shouldBeRemoved)
}

function removeOptionalDependencies<Manifest extends BaseManifest> (
  manifest: Manifest,
  shouldBeRemoved: (input: string) => boolean
): Manifest {
  for (const optionalDependency in manifest.optionalDependencies) {
    if (shouldBeRemoved(optionalDependency)) {
      delete manifest.optionalDependencies[optionalDependency]
      delete manifest.dependencies?.[optionalDependency]
    }
  }
  return manifest
}
