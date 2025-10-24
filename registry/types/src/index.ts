import { type PackageManifest } from '@pnpm/types'

export type PackageMetadata = PackageMeta

export type PackageMetadataWithTime = PackageMetaWithTime

export interface PackageMeta {
  name: string
  'dist-tags': Record<string, string>
  versions: Record<string, PackageInRegistry>
  time?: PackageMetaTime
  cachedAt?: number
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
  dist: {
    integrity?: string
    shasum: string
    tarball: string
    attestations?: {
      provenance?: {
        predicateType: string
      }
    }
  }
}
