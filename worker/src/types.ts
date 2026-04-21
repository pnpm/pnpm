import type { PackageFilesResponse } from '@pnpm/store.cafs-types'
import type { DependencyManifest } from '@pnpm/types'

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
  /**
   * Regex source matching the normalized relative path of files inside the tarball that
   * should be skipped. Matching happens after the tar parser strips the top-level directory
   * segment — i.e. against the same path form that is written to `filesIndex`.
   */
  ignoreFilePattern?: string
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
  safeToSkip?: boolean
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

export interface FetchAndWriteCafsMessage {
  type: 'fetch-and-write-cafs'
  registryUrl: string
  storeDir: string
  digests: Array<{ digest: string, size: number, executable: boolean }>
}
