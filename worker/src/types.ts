import { type PackageFilesResponse } from '@pnpm/cafs-types'
import { type DependencyManifest } from '@pnpm/types'

export interface PkgNameVersion {
  name?: string
  version?: string
}

export interface InitStoreMessage {
  type: 'init-store'
  storeDir: string
}

export interface TarballExtractMessage {
  type: 'extract'
  buffer: Buffer
  storeDir: string
  integrity?: string
  filesIndexFile: string
  readManifest?: boolean
  pkg?: PkgNameVersion
  appendManifest?: DependencyManifest
}

export interface LinkPkgMessage {
  type: 'link'
  storeDir: string
  packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone' | 'clone-or-copy'
  filesResponse: PackageFilesResponse
  sideEffectsCacheKey?: string | undefined
  targetDir: string
  requiresBuild?: boolean
  force: boolean
  keepModulesDir?: boolean
  disableRelinkLocalDirDeps?: boolean
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
  storeDir: string
  dir: string
  filesIndexFile: string
  sideEffectsCacheKey?: string
  readManifest?: boolean
  pkg?: PkgNameVersion
  appendManifest?: DependencyManifest
  files?: string[]
  includeNodeModules?: boolean
}

export interface ReadPkgFromCafsMessage {
  type: 'readPkgFromCafs'
  storeDir: string
  filesIndexFile: string
  readManifest: boolean
  verifyStoreIntegrity: boolean
  expectedPkg?: PkgNameVersion
  strictStorePkgContentCheck?: boolean
}

export interface HardLinkDirMessage {
  type: 'hardLinkDir'
  src: string
  destDirs: string[]
}
