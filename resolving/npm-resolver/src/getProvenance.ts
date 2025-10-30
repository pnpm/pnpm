import { type PackageInRegistry } from '@pnpm/registry.types'

type Provenance = boolean | 'trustedPublisher'

export function getProvenance (manifest: PackageInRegistry): Provenance | undefined {
  const provenance = manifest._npmUser?.trustedPublisher
    ? 'trustedPublisher'
    : !!manifest.dist?.attestations?.provenance
  return provenance || undefined
}
