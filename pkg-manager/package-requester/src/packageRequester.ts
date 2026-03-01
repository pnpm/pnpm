import { createReadStream, promises as fs } from 'fs'
import path from 'path'
import {
  getIndexFilePathInCafs as _getIndexFilePathInCafs,
  normalizeBundledManifest,
} from '@pnpm/store.cafs'
import { fetchingProgressLogger, progressLogger } from '@pnpm/core-loggers'
import { pickFetcher } from '@pnpm/pick-fetcher'
import { PnpmError } from '@pnpm/error'
import {
  type DirectoryFetcherResult,
  type Fetchers,
  type FetchOptions,
  type FetchResult,
} from '@pnpm/fetcher-base'
import { type Cafs } from '@pnpm/cafs-types'
import gfs from '@pnpm/graceful-fs'
import { logger } from '@pnpm/logger'
import { packageIsInstallable } from '@pnpm/package-is-installable'
import { loadJsonFile } from 'load-json-file'
import {
  type PlatformAssetResolution,
  type DirectoryResolution,
  type PreferredVersions,
  type Resolution,
  type ResolveFunction,
  type ResolveResult,
  type TarballResolution,
  type AtomicResolution,
} from '@pnpm/resolver-base'
import {
  type BundledManifest,
  type PkgRequestFetchResult,
  type FetchPackageToStoreFunction,
  type FetchPackageToStoreOptions,
  type GetFilesIndexFilePath,
  type PackageResponse,
  type PkgNameVersion,
  type RequestPackageFunction,
  type RequestPackageOptions,
  type WantedDependency,
} from '@pnpm/store-controller-types'
import { type DependencyManifest, type SupportedArchitectures } from '@pnpm/types'
import { type CustomFetcher } from '@pnpm/hooks.types'
import { depPathToFilename } from '@pnpm/dependency-path'
import {
  calcMaxWorkers,
  readPkgFromCafs as _readPkgFromCafs,
  type ReadPkgFromCafsOptions,
  type ReadPkgFromCafsResult,
} from '@pnpm/worker'
import { familySync } from 'detect-libc'
import PQueue from 'p-queue'
import pDefer, { type DeferredPromise } from 'p-defer'
import pShare from 'promise-share'
import { pick } from 'ramda'
import ssri from 'ssri'

let currentLibc: 'glibc' | 'musl' | undefined | null
function getLibcFamilySync () {
  if (currentLibc === undefined) {
    currentLibc = familySync() as unknown as typeof currentLibc
  }
  return currentLibc
}
const TARBALL_INTEGRITY_FILENAME = 'tarball-integrity'
const packageRequestLogger = logger('package-requester')


export function createPackageRequester (
  opts: {
    engineStrict?: boolean
    force?: boolean
    nodeVersion?: string
    pnpmVersion?: string
    resolve: ResolveFunction
    fetchers: Fetchers
    cafs: Cafs
    ignoreFile?: (filename: string) => boolean
    networkConcurrency?: number
    storeDir: string
    verifyStoreIntegrity: boolean
    virtualStoreDirMaxLength: number
    strictStorePkgContentCheck?: boolean
    customFetchers?: CustomFetcher[]
  }
): RequestPackageFunction & {
  fetchPackageToStore: FetchPackageToStoreFunction
  getFilesIndexFilePath: GetFilesIndexFilePath
  requestPackage: RequestPackageFunction
} {
  opts = opts || {}

  const networkConcurrency = opts.networkConcurrency ?? Math.min(64, Math.max(calcMaxWorkers() * 3, 16))
  const requestsQueue = new PQueue({
    concurrency: networkConcurrency,
  })

  const getIndexFilePathInCafs = _getIndexFilePathInCafs.bind(null, opts.storeDir)
  const fetch = fetcher.bind(null, opts.fetchers, opts.cafs, opts.customFetchers)
  const readPkgFromCafs = _readPkgFromCafs.bind(null, {
    storeDir: opts.storeDir,
    verifyStoreIntegrity: opts.verifyStoreIntegrity,
    strictStorePkgContentCheck: opts.strictStorePkgContentCheck,
  })
  const fetchPackageToStore = fetchToStore.bind(null, {
    readPkgFromCafs,
    fetch,
    fetchingLocker: new Map(),
    getIndexFilePathInCafs,
    requestsQueue: Object.assign(requestsQueue, {
      counter: 0,
      concurrency: networkConcurrency,
    }),
    storeDir: opts.storeDir,
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    strictStorePkgContentCheck: opts.strictStorePkgContentCheck,
  })
  const requestPackage = resolveAndFetch.bind(null, {
    engineStrict: opts.engineStrict,
    nodeVersion: opts.nodeVersion,
    pnpmVersion: opts.pnpmVersion,
    force: opts.force,
    fetchPackageToStore,
    requestsQueue,
    resolve: opts.resolve,
    storeDir: opts.storeDir,
  })

  return Object.assign(requestPackage, {
    fetchPackageToStore,
    getFilesIndexFilePath: getFilesIndexFilePath.bind(null, {
      getIndexFilePathInCafs,
      storeDir: opts.storeDir,
      virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    }),
    requestPackage,
  })
}

async function resolveAndFetch (
  ctx: {
    engineStrict?: boolean
    force?: boolean
    nodeVersion?: string
    pnpmVersion?: string
    requestsQueue: { add: <T>(fn: () => Promise<T>, opts: { priority: number }) => Promise<T> }
    resolve: ResolveFunction
    fetchPackageToStore: FetchPackageToStoreFunction
    storeDir: string
  },
  wantedDependency: WantedDependency & { optional?: boolean },
  options: RequestPackageOptions
): Promise<PackageResponse> {
  let resolution = options.currentPkg?.resolution as Resolution
  let pkgId = options.currentPkg?.id

  // When we have a currentPkg but a resolution is still performed due to
  // options.skipFetch, it's necessary to make sure the resolution doesn't
  // accidentally return a newer version of the package. When skipFetch is
  // set, the resolved package shouldn't be different. This is done by
  // overriding the preferredVersions object to only contain the current
  // package's version.
  //
  // A naive approach would be to change the bare specifier to be the exact
  // version of the current pkg if the bare specifier is a range, but this
  // would cause the version returned for calcSpecifier to be different.
  const preferredVersions: PreferredVersions = (resolution && !options.update && options.currentPkg?.name != null && options.currentPkg?.version != null)
    ? {
      ...options.preferredVersions,
      [options.currentPkg.name]: { [options.currentPkg.version]: 'version' },
    }
    : options.preferredVersions

  const resolveResult = await ctx.requestsQueue.add<ResolveResult>(async () => ctx.resolve(wantedDependency, {
    ...options,
    preferredVersions,
    currentPkg: (options.currentPkg?.id && options.currentPkg?.resolution)
      ? {
        id: options.currentPkg.id,
        name: options.currentPkg.name,
        version: options.currentPkg.version,
        resolution: options.currentPkg.resolution,
      }
      : undefined,
  }), { priority: options.downloadPriority })

  let { manifest } = resolveResult
  const {
    latest,
    resolvedVia,
    publishedAt,
    normalizedBareSpecifier,
    alias,
  } = resolveResult

  // Check if the integrity has changed between the current and newly resolved package
  // Use 'in' check to safely access integrity from any resolution type that has it
  const previousResolution = options.currentPkg?.resolution
  const previousIntegrity = previousResolution && 'integrity' in previousResolution ? previousResolution.integrity : undefined
  const newIntegrity = 'integrity' in resolveResult.resolution ? resolveResult.resolution.integrity : undefined
  const integrityChanged = previousIntegrity != null && newIntegrity != null && previousIntegrity !== newIntegrity

  const updated = pkgId !== resolveResult.id || !resolution || integrityChanged
  resolution = resolveResult.resolution
  pkgId = resolveResult.id

  const id = pkgId!

  if ('type' in resolution && resolution.type === 'directory' && !id.startsWith('file:')) {
    if (manifest == null) {
      throw new Error(`Couldn't read package.json of local dependency ${wantedDependency.alias ? wantedDependency.alias + '@' : ''}${wantedDependency.bareSpecifier ?? ''}`)
    }
    return {
      body: {
        id,
        isLocal: true,
        manifest,
        resolution: resolution as DirectoryResolution,
        resolvedVia,
        updated,
        normalizedBareSpecifier,
        alias,
      },
    }
  }

  let isInstallable: boolean | null | undefined = (
    ctx.force === true ||
    (
      manifest == null
        ? undefined
        : packageIsInstallable(id, manifest, {
          engineStrict: ctx.engineStrict,
          lockfileDir: options.lockfileDir,
          nodeVersion: ctx.nodeVersion,
          optional: wantedDependency.optional === true,
          supportedArchitectures: options.supportedArchitectures,
        })
    )
  )
  // We can skip fetching the package only if the manifest
  // is present after resolution AND the content of the package has not changed
  if ((options.skipFetch === true || isInstallable === false) && !integrityChanged && (manifest != null)) {
    return {
      body: {
        id,
        isLocal: false as const,
        isInstallable: isInstallable ?? undefined,
        latest,
        manifest,
        normalizedBareSpecifier,
        resolution,
        resolvedVia,
        updated,
        publishedAt,
        alias,
      },
    }
  }

  const pkg: PkgNameVersion = manifest != null ? pick(['name', 'version'], manifest) : {}
  const fetchResult = ctx.fetchPackageToStore({
    allowBuild: options.allowBuild,
    fetchRawManifest: true,
    force: integrityChanged,
    ignoreScripts: options.ignoreScripts,
    lockfileDir: options.lockfileDir,
    pkg: {
      ...(options.expectedPkg?.name != null
        ? (updated ? { name: options.expectedPkg.name, version: pkg.version } : options.expectedPkg)
        : pkg
      ),
      id,
      resolution,
    },
    onFetchError: options.onFetchError,
    supportedArchitectures: options.supportedArchitectures,
  })

  if (!manifest) {
    const fetchedResult = await fetchResult.fetching()
    if (fetchedResult.bundledManifest) {
      manifest = fetchedResult.bundledManifest as DependencyManifest
    } else if (fetchedResult.files.filesMap.has('package.json')) {
      manifest = await loadJsonFile<DependencyManifest>(fetchedResult.files.filesMap.get('package.json')!)
    }
    // Add integrity to resolution if it was computed during fetching (only for TarballResolution)
    if (fetchedResult.integrity && !resolution.type && !(resolution as TarballResolution).integrity) {
      (resolution as TarballResolution).integrity = fetchedResult.integrity
    }
  }
  // Check installability now that we have the manifest (for git/tarball packages without registry metadata)
  if (isInstallable === undefined && manifest != null) {
    isInstallable = ctx.force === true || packageIsInstallable(id, manifest, {
      engineStrict: ctx.engineStrict,
      lockfileDir: options.lockfileDir,
      nodeVersion: ctx.nodeVersion,
      optional: wantedDependency.optional === true,
      supportedArchitectures: options.supportedArchitectures,
    })
  }
  return {
    body: {
      id,
      isLocal: false as const,
      isInstallable: isInstallable ?? undefined,
      latest,
      manifest,
      normalizedBareSpecifier,
      resolution,
      resolvedVia,
      updated,
      publishedAt,
      alias,
    },
    fetching: fetchResult.fetching,
    filesIndexFile: fetchResult.filesIndexFile,
  }
}

interface FetchLock {
  fetching: Promise<PkgRequestFetchResult>
  filesIndexFile: string
  fetchRawManifest?: boolean
}

interface GetFilesIndexFilePathResult {
  target: string
  filesIndexFile: string
  resolution: AtomicResolution
}

function getFilesIndexFilePath (
  ctx: {
    getIndexFilePathInCafs: (integrity: string, pkgId: string) => string
    storeDir: string
    virtualStoreDirMaxLength: number
  },
  opts: Pick<FetchPackageToStoreOptions, 'pkg' | 'ignoreScripts' | 'supportedArchitectures'>
): GetFilesIndexFilePathResult {
  const targetRelative = depPathToFilename(opts.pkg.id, ctx.virtualStoreDirMaxLength)
  const target = path.join(ctx.storeDir, targetRelative)
  if ((opts.pkg.resolution as TarballResolution).integrity) {
    return {
      target,
      filesIndexFile: ctx.getIndexFilePathInCafs((opts.pkg.resolution as TarballResolution).integrity!, opts.pkg.id),
      resolution: opts.pkg.resolution as AtomicResolution,
    }
  }
  let resolution!: AtomicResolution
  if (opts.pkg.resolution.type === 'variations') {
    resolution = findResolution(opts.pkg.resolution.variants, opts.supportedArchitectures)
    if ((resolution as TarballResolution).integrity) {
      return {
        target,
        filesIndexFile: ctx.getIndexFilePathInCafs((resolution as TarballResolution).integrity!, opts.pkg.id),
        resolution,
      }
    }
  } else {
    resolution = opts.pkg.resolution
  }
  const filesIndexFile = path.join(target, opts.ignoreScripts ? 'integrity-not-built.mpk' : 'integrity.mpk')
  return { filesIndexFile, target, resolution }
}

function findResolution (resolutionVariants: PlatformAssetResolution[], supportedArchitectures?: SupportedArchitectures): AtomicResolution {
  const platform = getOneIfNonCurrent(supportedArchitectures?.os) ?? process.platform
  const cpu = getOneIfNonCurrent(supportedArchitectures?.cpu) ?? process.arch
  const libc = getOneIfNonCurrent(supportedArchitectures?.libc) ?? getLibcFamilySync()
  const resolutionVariant = resolutionVariants
    .find((resolutionVariant) => resolutionVariant.targets.some(
      (target) =>
        target.os === platform &&
        target.cpu === cpu &&
        (target.libc == null || target.libc === libc)
    ))
  if (!resolutionVariant) {
    const resolutionTargets = resolutionVariants.map((variant) => variant.targets)
    throw new PnpmError('NO_RESOLUTION_MATCHED', `Cannot find a resolution variant for the current platform in these resolutions: ${JSON.stringify(resolutionTargets)}`)
  }
  return resolutionVariant.resolution
}

function getOneIfNonCurrent (requirements: string[] | undefined): string | undefined {
  if (requirements?.length && requirements[0] !== 'current') {
    return requirements[0]
  }
  return undefined
}

function fetchToStore (
  ctx: {
    readPkgFromCafs: (
      filesIndexFile: string,
      opts?: ReadPkgFromCafsOptions
    ) => Promise<ReadPkgFromCafsResult>
    fetch: (
      packageId: string,
      resolution: AtomicResolution,
      opts: FetchOptions
    ) => Promise<FetchResult>
    fetchingLocker: Map<string, FetchLock>
    getIndexFilePathInCafs: (integrity: string, pkgId: string) => string
    requestsQueue: {
      add: <T>(fn: () => Promise<T>, opts: { priority: number }) => Promise<T>
      counter: number
      concurrency: number
    }
    storeDir: string
    virtualStoreDirMaxLength: number
    strictStorePkgContentCheck?: boolean
  },
  opts: FetchPackageToStoreOptions
): {
    filesIndexFile: string
    fetching: () => Promise<PkgRequestFetchResult>
  } {
  if (!opts.pkg.name) {
    opts.fetchRawManifest = true
  }

  if (!ctx.fetchingLocker.has(opts.pkg.id)) {
    const fetching = pDefer<PkgRequestFetchResult>()
    const { filesIndexFile, target, resolution } = getFilesIndexFilePath(ctx, opts)

    doFetchToStore(filesIndexFile, fetching, target, resolution)

    ctx.fetchingLocker.set(opts.pkg.id, {
      fetching: removeKeyOnFail(fetching.promise),
      filesIndexFile,
      fetchRawManifest: opts.fetchRawManifest,
    })

    // When files resolves, the cached result has to set fromStore to true, without
    // affecting previous invocations: so we need to replace the cache.
    //
    // Changing the value of fromStore is needed for correct reporting of `pnpm server`.
    // Otherwise, if a package was not in store when the server started, it will always be
    // reported as "downloaded" instead of "reused".
    fetching.promise.then((cache) => {
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

      const tmp = ctx.fetchingLocker.get(opts.pkg.id)

      // If fetching failed then it was removed from the cache.
      // It is OK. In that case there is no need to update it.
      if (tmp == null) return

      ctx.fetchingLocker.set(opts.pkg.id, {
        ...tmp,
        fetching: Promise.resolve({
          ...cache,
          files: {
            ...cache.files,
            resolvedFrom: 'store',
          },
        }),
      })
    })
      .catch(() => {
        ctx.fetchingLocker.delete(opts.pkg.id)
      })
  }

  const result = ctx.fetchingLocker.get(opts.pkg.id)!

  if (opts.fetchRawManifest && !result.fetchRawManifest) {
    result.fetching = removeKeyOnFail(
      result.fetching.then(async ({ files }) => {
        if (!files.filesMap.has('package.json')) return {
          files,
          bundledManifest: undefined,
        }
        return {
          files,
          bundledManifest: await readBundledManifest(files.filesMap.get('package.json')!),
        }
      })
    )
    result.fetchRawManifest = true
  }

  return {
    fetching: pShare(result.fetching),
    filesIndexFile: result.filesIndexFile,
  }

  async function removeKeyOnFail<T> (p: Promise<T>): Promise<T> {
    try {
      return await p
    } catch (err: any) { // eslint-disable-line
      ctx.fetchingLocker.delete(opts.pkg.id)
      if (opts.onFetchError) {
        throw opts.onFetchError(err)
      }
      throw err
    }
  }

  async function doFetchToStore (
    filesIndexFile: string,
    fetching: DeferredPromise<PkgRequestFetchResult>,
    target: string,
    resolution: AtomicResolution
  ) {
    try {
      const isLocalTarballDep = opts.pkg.id.startsWith('file:')
      const isLocalPkg = resolution.type === 'directory'

      if (
        !opts.force &&
        (
          !isLocalTarballDep ||
          await tarballIsUpToDate(opts.pkg.resolution as any, target, opts.lockfileDir) // eslint-disable-line
        ) &&
        !isLocalPkg
      ) {
        const { verified, files, bundledManifest } = await ctx.readPkgFromCafs(filesIndexFile, {
          readManifest: opts.fetchRawManifest,
          expectedPkg: opts.pkg,
        })
        if (verified) {
          fetching.resolve({
            files,
            bundledManifest,
          })
          return
        }
        if ((files?.filesMap) != null) {
          packageRequestLogger.warn({
            message: `Refetching ${target} to store. It was either modified or had no integrity checksums`,
            prefix: opts.lockfileDir,
          })
        }
      }

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
        {
          allowBuild: opts.allowBuild,
          filesIndexFile,
          lockfileDir: opts.lockfileDir,
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
      ), { priority })

      const integrity = (opts.pkg.resolution as TarballResolution).integrity ?? fetchedPackage.integrity
      if (isLocalTarballDep && integrity) {
        await fs.mkdir(target, { recursive: true })
        await gfs.writeFile(path.join(target, TARBALL_INTEGRITY_FILENAME), integrity, 'utf8')
      }

      fetching.resolve({
        files: {
          resolvedFrom: fetchedPackage.local ? 'local-dir' : 'remote',
          filesMap: fetchedPackage.filesMap,
          packageImportMethod: (fetchedPackage as DirectoryFetcherResult).packageImportMethod,
          requiresBuild: fetchedPackage.requiresBuild,
        },
        bundledManifest: fetchedPackage.manifest,
        integrity,
      })
    } catch (err: any) { // eslint-disable-line
      fetching.reject(err)
    }
  }
}

async function readBundledManifest (pkgJsonPath: string): Promise<BundledManifest | undefined> {
  return normalizeBundledManifest(await loadJsonFile<DependencyManifest>(pkgJsonPath))
}

async function tarballIsUpToDate (
  resolution: {
    integrity?: string
    registry?: string
    tarball: string
  },
  pkgInStoreLocation: string,
  lockfileDir: string
) {
  let currentIntegrity!: string
  try {
    currentIntegrity = (await gfs.readFile(path.join(pkgInStoreLocation, TARBALL_INTEGRITY_FILENAME), 'utf8'))
  } catch (err: any) { // eslint-disable-line
    return false
  }
  if (resolution.integrity && currentIntegrity !== resolution.integrity) return false

  const tarball = path.join(lockfileDir, resolution.tarball.slice(5))
  const tarballStream = createReadStream(tarball)
  try {
    return Boolean(await ssri.checkStream(tarballStream, currentIntegrity))
  } catch (err: any) { // eslint-disable-line
    return false
  }
}

async function fetcher (
  fetcherByHostingType: Fetchers,
  cafs: Cafs,
  customFetchers: CustomFetcher[] | undefined,
  packageId: string,
  resolution: AtomicResolution,
  opts: FetchOptions
): Promise<FetchResult> {
  try {
    // pickFetcher now handles custom fetcher hooks internally
    const fetch = await pickFetcher(fetcherByHostingType, resolution, {
      customFetchers,
      packageId,
    })
    const result = await fetch(cafs, resolution as any, opts) // eslint-disable-line @typescript-eslint/no-explicit-any
    return result
  } catch (err: any) { // eslint-disable-line
    packageRequestLogger.warn({
      message: `Fetching ${packageId} failed!`,
      prefix: opts.lockfileDir,
    })
    throw err
  }
}
