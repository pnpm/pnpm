import fs from 'node:fs'
import path from 'node:path'

import { PnpmError } from '@pnpm/error'
import type { Fetchers } from '@pnpm/fetching.fetcher-base'
import type { CustomFetcher } from '@pnpm/hooks.types'
import { createPackageRequester } from '@pnpm/installing.package-requester'
import type { ResolveFunction } from '@pnpm/resolving.resolver-base'
import { type PackageFilesIndex, verifyFileIntegrityAsync } from '@pnpm/store.cafs'
import type {
  ImportIndexedPackageAsync,
  SideEffectsDiff,
  StoreController,
} from '@pnpm/store.controller-types'
import { type CafsLocker, createCafsStore, createPackageImporterAsync } from '@pnpm/store.create-cafs-store'
import type { StoreIndex } from '@pnpm/store.index'
import { addFilesFromDir, importPackage, initStoreDir } from '@pnpm/worker'

import { prune } from './prune.js'

type Cafs = ReturnType<typeof createCafsStore>

const MAX_QUARANTINED_REMOTE_SIDE_EFFECTS = 64

export { type CafsLocker }

export interface CreatePackageStoreOptions {
  cafsLocker?: CafsLocker
  engineStrict?: boolean
  force?: boolean
  forceIgnoresPlatform?: boolean
  nodeVersion?: string
  importPackage?: ImportIndexedPackageAsync
  pnpmVersion?: string
  ignoreFile?: (filename: string) => boolean
  cacheDir: string
  storeDir: string
  networkConcurrency?: number
  packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone' | 'clone-or-copy'
  verifyStoreIntegrity: boolean
  virtualStoreDirMaxLength: number
  strictStorePkgContentCheck?: boolean
  clearResolutionCache: () => void
  customFetchers?: CustomFetcher[]
  frozenStore?: boolean
  storeIndex: StoreIndex
}

export function createPackageStore (
  resolve: ResolveFunction,
  fetchers: Fetchers,
  initOpts: CreatePackageStoreOptions
): StoreController {
  const storeDir = initOpts.storeDir
  ensureStoreContentDir(storeDir, initOpts.frozenStore)
  const cafs = createCafsStore(storeDir, {
    cafsLocker: initOpts.cafsLocker,
    packageImportMethod: initOpts.packageImportMethod,
  })
  const packageRequester = createStorePackageRequester(initOpts, { resolve, fetchers, cafs })

  return {
    hasCustomFetchers: (initOpts.customFetchers?.length ?? 0) > 0,
    close: async () => {
      initOpts.storeIndex.flush()
    },
    fetchPackage: packageRequester.fetchPackageToStore,
    getFilesIndexFilePath: packageRequester.getFilesIndexFilePath,
    importPackage: createImportPackage(initOpts, cafs),
    prune: prune.bind(null, { storeDir, cacheDir: initOpts.cacheDir, storeIndex: initOpts.storeIndex }),
    requestPackage: packageRequester.requestPackage,
    upload: uploadToStore.bind(null, { storeDir: cafs.storeDir, storeIndex: initOpts.storeIndex }),
    // A read-only store cannot accept new content, so it does not advertise the
    // direct write capability that remote side-effects hydration requires.
    addFileToStore: initOpts.frozenStore ? undefined : cafs.addFile,
    locateFileInStore: locateFileInStore.bind(null, cafs),
    persistRemoteSideEffects: initOpts.frozenStore ? undefined : persistRemoteSideEffects.bind(null, initOpts.storeIndex),
    quarantineRemoteSideEffects: initOpts.frozenStore ? undefined : quarantineRemoteSideEffects.bind(null, initOpts.storeIndex),
    clearResolutionCache: initOpts.clearResolutionCache,
  }
}

function createStorePackageRequester (
  initOpts: CreatePackageStoreOptions,
  dependencies: Pick<Parameters<typeof createPackageRequester>[0], 'resolve' | 'fetchers' | 'cafs'>
): ReturnType<typeof createPackageRequester> {
  return createPackageRequester({
    force: initOpts.force,
    forceIgnoresPlatform: initOpts.forceIgnoresPlatform,
    engineStrict: initOpts.engineStrict,
    nodeVersion: initOpts.nodeVersion,
    pnpmVersion: initOpts.pnpmVersion,
    ...dependencies,
    ignoreFile: initOpts.ignoreFile,
    networkConcurrency: initOpts.networkConcurrency,
    storeDir: initOpts.storeDir,
    verifyStoreIntegrity: initOpts.verifyStoreIntegrity,
    virtualStoreDirMaxLength: initOpts.virtualStoreDirMaxLength,
    strictStorePkgContentCheck: initOpts.strictStorePkgContentCheck,
    customFetchers: initOpts.customFetchers,
    frozenStore: initOpts.frozenStore,
  })
}

function ensureStoreContentDir (storeDir: string, frozenStore: boolean | undefined): void {
  if (fs.existsSync(path.join(storeDir, 'files'))) return
  // A missing `{storeDir}/files` means the store has no content directory yet.
  // Under frozenStore the store is meant to be a complete, read-only seed, so
  // this is a setup error: initializing it would be a write into a read-only
  // store. Fail fast with actionable guidance instead of swallowing the write.
  if (frozenStore) {
    throw new PnpmError('FROZEN_STORE_INCOMPLETE',
      `frozenStore is enabled but the store at ${storeDir} is missing its content directory (${path.join(storeDir, 'files')}). The store must be fully seeded before it can be used read-only.`)
  }
  initStoreDir(storeDir).catch(() => {})
}

function createImportPackage (
  initOpts: CreatePackageStoreOptions,
  cafs: Cafs
): StoreController['importPackage'] {
  if (initOpts.importPackage) {
    return createPackageImporterAsync({ importIndexedPackage: initOpts.importPackage, storeDir: cafs.storeDir })
  }
  return (targetDir, opts) => importPackage({
    ...opts,
    packageImportMethod: initOpts.packageImportMethod,
    storeDir: initOpts.storeDir,
    targetDir,
  })
}

async function locateFileInStore (cafs: Cafs, hexDigest: string, mode: number): Promise<string | undefined> {
  const filePath = cafs.getFilePathByModeInCafs(hexDigest, mode)
  // Verified unconditionally rather than answering to `verifyStoreIntegrity`:
  // the download this skips would have ended in a CAS write, and that path
  // checks content already at the destination whatever the setting says.
  // Hashing a local file is far cheaper than the transfer it avoids.
  return await verifyFileIntegrityAsync(filePath, { algorithm: 'sha512', digest: hexDigest })
    ? filePath
    : undefined
}

function persistRemoteSideEffects (storeIndex: StoreIndex, opts: {
  filesIndexFile: string
  sideEffectsCacheKey: string
  sideEffects: SideEffectsDiff
}): boolean {
  return storeIndex.update(opts.filesIndexFile, (value) => {
    const index = value as PackageFilesIndex
    index.sideEffects ??= new Map()
    index.sideEffects.set(opts.sideEffectsCacheKey, opts.sideEffects)
    return index
  })
}

function quarantineRemoteSideEffects (storeIndex: StoreIndex, opts: {
  channel: string
  envelopeDigest: string
  filesIndexFile: string
}): boolean {
  return storeIndex.update(opts.filesIndexFile, (value) => {
    const index = value as PackageFilesIndex
    index.remoteSideEffectsQuarantine ??= new Map()
    const quarantined = index.remoteSideEffectsQuarantine.get(opts.channel) ?? []
    const bounded = Array.from(new Set([...quarantined, opts.envelopeDigest]))
      .slice(-MAX_QUARANTINED_REMOTE_SIDE_EFFECTS)
    index.remoteSideEffectsQuarantine.set(opts.channel, bounded)
    return index
  })
}

async function uploadToStore (
  store: { storeDir: string, storeIndex: StoreIndex },
  builtPkgLocation: string,
  opts: { filesIndexFile: string, sideEffectsCacheKey: string }
) {
  const result = await addFilesFromDir({
    storeDir: store.storeDir,
    storeIndex: store.storeIndex,
    dir: builtPkgLocation,
    sideEffectsCacheKey: opts.sideEffectsCacheKey,
    filesIndexFile: opts.filesIndexFile,
    pkg: {},
  })
  return {
    filesMap: result.filesMap,
    sideEffects: result.sideEffects,
  }
}
