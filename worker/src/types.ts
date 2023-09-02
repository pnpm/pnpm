import { type PackageFilesResponse } from '@pnpm/cafs-types'

export interface PkgNameVersion {
  name?: string
  version?: string
}

export interface TarballExtractMessage {
  type: 'extract'
  buffer: Buffer
  cafsDir: string
  integrity?: string
  filesIndexFile: string
  readManifest?: boolean
  pkg?: PkgNameVersion
}

export interface LinkPkgMessage {
  type: 'link'
  storeDir: string
  packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone' | 'clone-or-copy'
  filesResponse: PackageFilesResponse
  sideEffectsCacheKey?: string | undefined
  targetDir: string
  requiresBuild: boolean
  force: boolean
  keepModulesDir?: boolean
}

export interface AddDirToStoreMessage {
  type: 'add-dir'
  cafsDir: string
  dir: string
  filesIndexFile: string
  sideEffectsCacheKey?: string
  readManifest?: boolean
  pkg?: PkgNameVersion
}

export interface ReadPkgFromCafsMessage {
  type: 'readPkgFromCafs'
  cafsDir: string
  filesIndexFile: string
  readManifest: boolean
  verifyStoreIntegrity: boolean
}
