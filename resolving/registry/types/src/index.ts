import type { PackageManifest } from '@pnpm/types'

export type PackageMetadata = PackageMeta

export type PackageMetadataWithTime = PackageMetaWithTime

export interface PackageMeta {
  name: string
  'dist-tags': Record<string, string>
  versions: Record<string, PackageInRegistry>
  time?: PackageMetaTime
  modified?: string
  cachedAt?: number
  etag?: string
}

export interface PackageMetaWithTime extends PackageMeta {
  time: PackageMetaTime
}

export type PackageMetaTime = Record<string, string> & {
  unpublished?: {
    time: string
    versions: string[]
  }
}

export interface PackageInRegistry extends PackageManifest {
  hasInstallScript?: boolean
  _npmUser?: {
    name?: string
    email?: string
    trustedPublisher?: {
      id: string
      oidcConfigId: string
    }
  }
  maintainers?: Array<{
    name: string
    email?: string
    url?: string
  }>
  contributors?: Array<{
    name: string
    email?: string
    url?: string
  }>
  dist: {
    integrity?: string
    shasum: string
    tarball: string
    unpackedSize?: number
    attestations?: {
      provenance?: {
        predicateType: string
      }
    }
  }
}
