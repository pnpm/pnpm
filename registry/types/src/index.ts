import { type PackageManifest } from '@pnpm/types'

export type PackageDocument = PackageMeta

export type PackageDocumentWithTime = PackageMetaWithTime

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
  dist: {
    integrity?: string
    shasum: string
    tarball: string
  }
}
