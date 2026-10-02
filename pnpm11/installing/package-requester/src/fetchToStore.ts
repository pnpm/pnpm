import { promises as fs } from 'node:fs'
import path from 'node:path'

import { fetchingProgressLogger, progressLogger } from '@pnpm/core-loggers'
import type {
  DirectoryFetcherResult,
  FetchOptions,
  FetchResult,
} from '@pnpm/fetching.fetcher-base'
import type { PickedFetcher } from '@pnpm/fetching.pick-fetcher'
import gfs from '@pnpm/fs.graceful-fs'
import {
  type AtomicResolution,
  classifyResolution,
} from '@pnpm/resolving.resolver-base'
import type {
  FetchPackageToStoreOptions,
  PkgRequestFetchResult,
} from '@pnpm/store.controller-types'
import { gitHostedStoreIndexKey } from '@pnpm/store.index'
import type {
  ReadPkgFromCafsOptions,
  ReadPkgFromCafsResult,
} from '@pnpm/worker'
import pDefer, { type DeferredPromise } from 'p-defer'
import { pShare } from 'promise-share'

import { packageRequestLogger } from './fetcher.js'
import { getFilesIndexFilePath, type GetFilesIndexFilePathResult, isGitResolutionKind } from './getFilesIndexFilePath.js'
import {
  assertFetchableResolution,
  cachedPackageCanBeReused,
  getExpectedIntegrity,
  readBundledManifest,
  TARBALL_INTEGRITY_FILENAME,
  tarballIsUpToDate,
} from './storeEntry.js'


export interface FetchLock {
  ignoredBuild?: boolean
  fetching: Promise<PkgRequestFetchResult>
  filesIndexFile: string
  fetchRawManifest?: boolean
}

export interface FetchToStoreContext {
  readPkgFromCafs: (
    filesIndexFile: string,
    opts?: ReadPkgFromCafsOptions
  ) => Promise<ReadPkgFromCafsResult>
  fetch: (
    packageId: string,
    resolution: AtomicResolution,
    opts: FetchOptions,
    pickedFetcher?: PickedFetcher
  ) => Promise<FetchResult>
  fetchingLocker: Map<string, FetchLock>
  requestsQueue: {
    add: <Result>(fn: () => Promise<Result>, opts: { priority: number }) => Promise<Result>
    counter: number
    concurrency: number
  }
  storeDir: string
  virtualStoreDirMaxLength: number
  strictStorePkgContentCheck?: boolean
}

export interface FetchToStoreResult {
  filesIndexFile: string
  fetching: () => Promise<PkgRequestFetchResult>
}

type ResolutionKind = ReturnType<typeof classifyResolution>

interface FetchRequest {
  ctx: FetchToStoreContext
  opts: FetchPackageToStoreOptions
  fetchingKey: string
  resolutionKind: ResolutionKind
}

export function fetchToStore (
  ctx: FetchToStoreContext,
  opts: FetchPackageToStoreOptions
): FetchToStoreResult {
  if (!opts.pkg.name) {
    opts.fetchRawManifest = true
  }

  const location = getFilesIndexFilePath(ctx, opts)
  const resolutionKind = classifyResolution(location.resolution)
  const request: FetchRequest = {
    ctx,
    opts,
    fetchingKey: isGitResolutionKind(resolutionKind)
      ? `${opts.lockfileDir}\0${opts.pkg.id}\0${location.filesIndexFile}`
      : opts.pkg.id,
    resolutionKind,
  }
  const reusingPolicySensitiveFetch = ctx.fetchingLocker.has(request.fetchingKey) && isGitResolutionKind(resolutionKind)

  if (!ctx.fetchingLocker.has(request.fetchingKey)) {
    startFetching(request, location)
  }

  const fetchLock = ctx.fetchingLocker.get(request.fetchingKey)!

  if (opts.fetchRawManifest && !fetchLock.fetchRawManifest) {
    fetchLock.fetching = removeKeyOnFail(request, fetchLock.fetching.then(addBundledManifest))
    fetchLock.fetchRawManifest = true
  }

  if (reusingPolicySensitiveFetch) {
    return reuseFetchIfBuildPolicyAllows(request, fetchLock)
  }
  return {
    fetching: pShare(fetchLock.fetching),
    get filesIndexFile () {
      return fetchLock.filesIndexFile
    },
  }
}

function startFetching (request: FetchRequest, location: GetFilesIndexFilePathResult): void {
  const { ctx, opts, fetchingKey } = request
  const fetching = pDefer<PkgRequestFetchResult>()

  // doFetchToStore never rejects, its errors reject `fetching`.
  void doFetchToStore(request, fetching, location)

  ctx.fetchingLocker.set(fetchingKey, {
    fetching: removeKeyOnFail(request, fetching.promise),
    filesIndexFile: location.filesIndexFile,
    fetchRawManifest: opts.fetchRawManifest,
  })

  // When files resolves, the cached result has to set fromStore to true, without
  // affecting previous invocations: so we need to replace the cache.
  //
  // Changing the value of fromStore is needed for correct reporting of `pnpm server`.
  // Otherwise, if a package was not in store when the server started, it will always be
  // reported as "downloaded" instead of "reused".
  fetching.promise.then((cache) => {
    markFetchedPackageAsStored(request, cache)
  })
    .catch(() => {
      ctx.fetchingLocker.delete(fetchingKey)
    })
}

function markFetchedPackageAsStored ({ ctx, opts, fetchingKey }: FetchRequest, cache: PkgRequestFetchResult): void {
  progressLogger.debug({
    packageId: opts.pkg.id,
    requester: opts.lockfileDir,
    status: cache.files.resolvedFrom === 'remote'
      ? 'fetched'
      : 'found_in_store',
  })

  // If it's already in the store, we don't need to update the cache
  if (cache.files.resolvedFrom !== 'remote') {
    return
  }

  const fetchLock = ctx.fetchingLocker.get(fetchingKey)

  // If fetching failed then it was removed from the cache.
  // It is OK. In that case there is no need to update it.
  if (fetchLock == null) return

  ctx.fetchingLocker.set(fetchingKey, {
    ...fetchLock,
    fetching: Promise.resolve({
      ...cache,
      files: {
        ...cache.files,
        resolvedFrom: 'store',
      },
    }),
  })
}

async function addBundledManifest ({ files }: PkgRequestFetchResult): Promise<PkgRequestFetchResult> {
  if (!files.filesMap.has('package.json')) return {
    files,
    bundledManifest: undefined,
  }
  return {
    files,
    bundledManifest: await readBundledManifest(files.filesMap.get('package.json')!),
  }
}

function reuseFetchIfBuildPolicyAllows (request: FetchRequest, fetchLock: FetchLock): FetchToStoreResult {
  const { ctx, opts, fetchingKey } = request
  let filesIndexResult: { filesIndexFile: string } = fetchLock
  const fetching = removeKeyOnFail(request, fetchLock.fetching.then(async (cached) => {
    if (await cachedPackageCanBeReused({
      allowBuild: opts.allowBuild,
      bundledManifest: cached.bundledManifest,
      filesMap: cached.files.filesMap,
      filesIndexFile: fetchLock.filesIndexFile,
      ignoredBuild: fetchLock.ignoredBuild,
      ignoreScripts: opts.ignoreScripts,
      pkgResolutionId: opts.pkg.id,
      requiresPrepare: cached.files.requiresPrepare,
      resolutionKind: request.resolutionKind,
    })) return cached
    if (ctx.fetchingLocker.get(fetchingKey) === fetchLock) {
      ctx.fetchingLocker.delete(fetchingKey)
    }
    const replacement = fetchToStore(ctx, opts)
    filesIndexResult = replacement
    return replacement.fetching()
  }))
  return {
    fetching: pShare(fetching),
    get filesIndexFile () {
      return filesIndexResult.filesIndexFile
    },
  }
}

async function removeKeyOnFail<Result> ({ ctx, opts, fetchingKey }: FetchRequest, fetchingPromise: Promise<Result>): Promise<Result> {
  try {
    return await fetchingPromise
  } catch (err: any) { // eslint-disable-line
    ctx.fetchingLocker.delete(fetchingKey)
    if (opts.onFetchError) {
      throw opts.onFetchError(err)
    }
    throw err
  }
}

async function doFetchToStore (
  request: FetchRequest,
  fetching: DeferredPromise<PkgRequestFetchResult>,
  location: GetFilesIndexFilePathResult
): Promise<void> {
  try {
    fetching.resolve(await fetchPackageFiles(request, location))
  } catch (err: any) { // eslint-disable-line
    fetching.reject(err)
  }
}

async function fetchPackageFiles (request: FetchRequest, location: GetFilesIndexFilePathResult): Promise<PkgRequestFetchResult> {
  const { ctx, opts } = request
  if (opts.populateMissingIntegrity !== true) {
    assertFetchableResolution(opts.pkg.id, location.resolution)
  }

  let refetchingStoredPackage = false

  if (await storeMayHoldPackage(opts, location)) {
    const storeEntry = await findStoreEntry(request, location.filesIndexFile)
    if (storeEntry.reusable) {
      const fetchLock = ctx.fetchingLocker.get(request.fetchingKey)
      if (fetchLock) fetchLock.filesIndexFile = storeEntry.filesIndexFile
      return { files: storeEntry.files, bundledManifest: storeEntry.bundledManifest }
    }
    refetchingStoredPackage = storeEntry.refetching
  }

  if (refetchingStoredPackage) {
    packageRequestLogger.warn({
      message: `Refetching ${location.target} to store. It was either modified or had no integrity checksums`,
      prefix: opts.lockfileDir,
    })
  }

  return fetchPackageFromRemote(request, location)
}

async function storeMayHoldPackage (opts: FetchPackageToStoreOptions, location: GetFilesIndexFilePathResult): Promise<boolean> {
  if (opts.force || opts.populateMissingIntegrity === true) return false
  if (
    opts.pkg.id.startsWith('file:') &&
    !await tarballIsUpToDate(opts.pkg.resolution as any, location.target, opts.lockfileDir) // eslint-disable-line
  ) return false
  return location.resolution.type !== 'directory'
}

async function findStoreEntry (request: FetchRequest, filesIndexFile: string) {
  const { opts, resolutionKind } = request
  let storeEntry = await readStoreEntry(request, filesIndexFile)
  let refetching = isModifiedStoreEntry(storeEntry)
  if (!storeEntry.reusable && !opts.pkg.name && !opts.ignoreScripts && isGitResolutionKind(resolutionKind)) {
    storeEntry = await readStoreEntry(request, gitHostedStoreIndexKey(opts.pkg.id, { built: filesIndexFile.endsWith('\tnot-built') }))
    refetching ||= isModifiedStoreEntry(storeEntry)
  }
  return { ...storeEntry, refetching }
}

function isModifiedStoreEntry (storeEntry: Pick<ReadPkgFromCafsResult, 'verified' | 'files'>): boolean {
  return !storeEntry.verified && storeEntry.files?.filesMap != null
}

async function readStoreEntry ({ ctx, opts, resolutionKind }: FetchRequest, candidateKey: string) {
  const { verified, files, bundledManifest } = await ctx.readPkgFromCafs(candidateKey, {
    readManifest: opts.fetchRawManifest,
    expectedPkg: opts.pkg,
  })
  const reusable = verified && await cachedPackageCanBeReused({
    allowBuild: opts.allowBuild,
    bundledManifest,
    filesMap: files.filesMap,
    filesIndexFile: candidateKey,
    ignoreScripts: opts.ignoreScripts,
    pkgResolutionId: opts.pkg.id,
    requiresPrepare: files.requiresPrepare,
    resolutionKind,
  })
  return { filesIndexFile: candidateKey, verified, files, bundledManifest, reusable }
}

async function fetchPackageFromRemote (
  { ctx, opts, fetchingKey }: FetchRequest,
  { filesIndexFile, target, resolution }: GetFilesIndexFilePathResult
): Promise<PkgRequestFetchResult> {
  // We fetch into targetStage directory first and then fs.rename() it to the
  // target directory.

  // Tarballs are requested first because they are bigger than metadata files.
  // However, when one line is left available, allow it to be picked up by a metadata request.
  // This is done in order to avoid situations when tarballs are downloaded in chunks
  // As many tarballs should be downloaded simultaneously as possible.
  const priority = (++ctx.requestsQueue.counter % ctx.requestsQueue.concurrency === 0 ? -1 : 1) * 1000

  const fetchedPackage = await ctx.requestsQueue.add(async () => ctx.fetch(
    opts.pkg.id,
    resolution,
    createFetchOptions(opts, filesIndexFile),
    opts.pickedFetcher
  ), { priority })

  const fetchLock = ctx.fetchingLocker.get(fetchingKey)
  if (fetchLock) {
    fetchLock.ignoredBuild = fetchedPackage.ignoredBuild
    fetchLock.filesIndexFile = fetchedPackage.filesIndexFile ?? filesIndexFile
  }

  const integrity = getExpectedIntegrity(opts.pkg.resolution) ?? fetchedPackage.integrity
  if (opts.pkg.id.startsWith('file:') && integrity) {
    await fs.mkdir(target, { recursive: true })
    await gfs.writeFile(path.join(target, TARBALL_INTEGRITY_FILENAME), integrity, 'utf8')
  }

  return {
    files: {
      resolvedFrom: fetchedPackage.local ? 'local-dir' : 'remote',
      filesMap: fetchedPackage.filesMap,
      packageImportMethod: (fetchedPackage as DirectoryFetcherResult).packageImportMethod,
      requiresBuild: fetchedPackage.requiresBuild,
      requiresPrepare: fetchedPackage.requiresPrepare,
      sourceExists: (fetchedPackage as DirectoryFetcherResult).sourceExists,
    },
    bundledManifest: fetchedPackage.manifest,
    integrity,
  }
}

function createFetchOptions (opts: FetchPackageToStoreOptions, filesIndexFile: string): FetchOptions {
  return {
    allowBuild: opts.allowBuild,
    filesIndexFile,
    lockfileDir: opts.lockfileDir,
    pkgResolutionId: opts.pkg.id,
    readManifest: opts.fetchRawManifest,
    onProgress: (downloaded) => {
      fetchingProgressLogger.debug({
        downloaded,
        packageId: opts.pkg.id,
        status: 'in_progress',
      })
    },
    onStart: (size, attempt) => {
      fetchingProgressLogger.debug({
        attempt,
        packageId: opts.pkg.id,
        size,
        status: 'started',
      })
    },
    pkg: {
      name: opts.pkg.name,
      version: opts.pkg.version,
    },
  }
}
