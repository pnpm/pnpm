import type { PackageFilesResponse } from '@pnpm/cafs-types'

export interface PkgNameVersion {
  name?: string | undefined
  version?: string | undefined
}

export interface TarballExtractMessage {
  type: 'extract'
  buffer: Buffer
  cafsDir: string
  integrity?: string | undefined
  filesIndexFile: string
  readManifest?: boolean | undefined
  pkg?: PkgNameVersion | undefined
}

export interface LinkPkgMessage {
  type: 'link'
  storeDir: string
  packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone' | 'clone-or-copy' | undefined
  filesResponse: PackageFilesResponse
  sideEffectsCacheKey?: string | undefined
  targetDir: string
  requiresBuild?: boolean | undefined
  force: boolean
  keepModulesDir?: boolean | undefined
  disableRelinkLocalDirDeps?: boolean | undefined
}

export interface SymlinkAllModulesMessage {
  type: 'symlinkAllModules'
  deps: Array<{
    children: Record<string, string>
    modules: string
    name: string
  }>
}

export interface AddDirToStoreMessage {
  type: 'add-dir'
  cafsDir: string
  dir: string
  filesIndexFile: string
  sideEffectsCacheKey?: string | undefined
  readManifest?: boolean | undefined
  pkg?: PkgNameVersion | undefined
}

export interface ReadPkgFromCafsMessage {
  type: 'readPkgFromCafs'
  cafsDir: string
  filesIndexFile: string
  readManifest: boolean
  verifyStoreIntegrity: boolean
}

export interface HardLinkDirMessage {
  type: 'hardLinkDir'
  src: string
  destDirs: string[]
}
