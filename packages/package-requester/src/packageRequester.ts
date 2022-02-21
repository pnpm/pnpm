import { createReadStream, promises as fs } from 'fs'
import path from 'path'
import {
  checkFilesIntegrity as _checkFilesIntegrity,
  FileType,
  getFilePathByModeInCafs as _getFilePathByModeInCafs,
  getFilePathInCafs as _getFilePathInCafs,
  PackageFileInfo,
  PackageFilesIndex,
} from '@pnpm/cafs'
import { fetchingProgressLogger, progressLogger } from '@pnpm/core-loggers'
import PnpmError from '@pnpm/error'
import {
  Cafs,
  DeferredManifestPromise,
  FetchFunction,
  FetchOptions,
  FetchResult,
  PackageFilesResponse,
} from '@pnpm/fetcher-base'
import gfs from '@pnpm/graceful-fs'
import logger from '@pnpm/logger'
import packageIsInstallable from '@pnpm/package-is-installable'
import readPackage from '@pnpm/read-package-json'
import {
  DirectoryResolution,
  Resolution,
  ResolveFunction,
  ResolveResult,
} from '@pnpm/resolver-base'
import {
  BundledManifest,
  FetchPackageToStoreFunction,
  PackageResponse,
  RequestPackageFunction,
  RequestPackageOptions,
  WantedDependency,
} from '@pnpm/store-controller-types'
import { DependencyManifest } from '@pnpm/types'
import { depPathToFilename } from 'dependency-path'
import PQueue from 'p-queue'
import loadJsonFile from 'load-json-file'
import pDefer from 'p-defer'
import pathTemp from 'path-temp'
import pShare from 'promise-share'
import pick from 'ramda/src/pick'
import renameOverwrite from 'rename-overwrite'
import semver from 'semver'
import ssri from 'ssri'
import equalOrSemverEqual from './equalOrSemverEqual'
import safeDeferredPromise from './safeDeferredPromise'

const TARBALL_INTEGRITY_FILENAME = 'tarball-integrity'
const packageRequestLogger = logger('package-requester')

const pickBundledManifest = pick([
  'bin',
  'bundledDependencies',
  'bundleDependencies',
  'dependencies',
  'directories',
  'engines',
  'name',
  'optionalDependencies',
  'os',
  'peerDependencies',
  'peerDependenciesMeta',
  'scripts',
  'version',
])

function normalizeBundledManifest (manifest: DependencyManifest): BundledManifest {
  return {
    ...pickBundledManifest(manifest),
    version: semver.clean(manifest.version ?? '0.0.0', { loose: true }) ?? manifest.version,
  }
}

export default function (
  opts: {
    engineStrict?: boolean
    force?: boolean
    nodeVersion?: string
    pnpmVersion?: string
    resolve: ResolveFunction
    fetchers: {[type: string]: FetchFunction}
    cafs: Cafs
    ignoreFile?: (filename: string) => boolean
    networkConcurrency?: number
    storeDir: string
    verifyStoreIntegrity: boolean
  }
): RequestPackageFunction & {
    fetchPackageToStore: FetchPackageToStoreFunction
    requestPackage: RequestPackageFunction
  } {
  opts = opts || {}

  const networkConcurrency = opts.networkConcurrency ?? 16
  const requestsQueue = new PQueue({
    concurrency: networkConcurrency,
  })
  requestsQueue['counter'] = 0 // eslint-disable-line
  requestsQueue['concurrency'] = networkConcurrency // eslint-disable-line

  const cafsDir = path.join(opts.storeDir, 'files')
  const getFilePathInCafs = _getFilePathInCafs.bind(null, cafsDir)
  const fetch = fetcher.bind(null, opts.fetchers, opts.cafs)
  const fetchPackageToStore = fetchToStore.bind(null, {
    checkFilesIntegrity: _checkFilesIntegrity.bind(null, cafsDir),
    fetch,
    fetchingLocker: new Map(),
    getFilePathByModeInCafs: _getFilePathByModeInCafs.bind(null, cafsDir),
    getFilePathInCafs,
    requestsQueue,
    storeDir: opts.storeDir,
    verifyStoreIntegrity: opts.verifyStoreIntegrity,
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
    verifyStoreIntegrity: opts.verifyStoreIntegrity,
  })

  return Object.assign(requestPackage, { fetchPackageToStore, requestPackage })
}

async function resolveAndFetch (
  ctx: {
    engineStrict?: boolean
    force?: boolean
    nodeVersion?: string
    pnpmVersion?: string
    requestsQueue: {add: <T>(fn: () => Promise<T>, opts: {priority: number}) => Promise<T>}
    resolve: ResolveFunction
    fetchPackageToStore: FetchPackageToStoreFunction
    storeDir: string
    verifyStoreIntegrity: boolean
  },
  wantedDependency: WantedDependency & { optional?: boolean },
  options: RequestPackageOptions
): Promise<PackageResponse> {
  let latest: string | undefined
  let manifest: DependencyManifest | undefined
  let normalizedPref: string | undefined
  let resolution = options.currentPkg?.resolution as Resolution
  let pkgId = options.currentPkg?.id
  const skipResolution = resolution && !options.update
  let forceFetch = false
  let updated = false
  let resolvedVia: string | undefined

  // When fetching is skipped, resolution cannot be skipped.
  // We need the package's manifest when doing `lockfile-only` installs.
  // When we don't fetch, the only way to get the package's manifest is via resolving it.
  //
  // The resolution step is never skipped for local dependencies.
  if (!skipResolution || options.skipFetch === true || Boolean(pkgId?.startsWith('file:')) || wantedDependency.optional === true) {
    const resolveResult = await ctx.requestsQueue.add<ResolveResult>(async () => ctx.resolve(wantedDependency, {
      alwaysTryWorkspacePackages: options.alwaysTryWorkspacePackages,
      defaultTag: options.defaultTag,
      lockfileDir: options.lockfileDir,
      preferredVersions: options.preferredVersions,
      preferWorkspacePackages: options.preferWorkspacePackages,
      projectDir: options.projectDir,
      registry: options.registry,
      workspacePackages: options.workspacePackages,
    }), { priority: options.downloadPriority })

    manifest = resolveResult.manifest
    latest = resolveResult.latest
    resolvedVia = resolveResult.resolvedVia

    // If the integrity of a local tarball dependency has changed,
    // the local tarball should be unpacked, so a fetch to the store should be forced
    forceFetch = Boolean(
      ((options.currentPkg?.resolution) != null) &&
      pkgId?.startsWith('file:') &&
      options.currentPkg?.resolution['integrity'] !== resolveResult.resolution['integrity'] // eslint-disable-line @typescript-eslint/dot-notation
    )

    updated = pkgId !== resolveResult.id || !resolution || forceFetch
    resolution = resolveResult.resolution
    pkgId = resolveResult.id
    normalizedPref = resolveResult.normalizedPref
  }

  const id = pkgId as string

  if (resolution.type === 'directory' && !wantedDependency.injected) {
    if (manifest == null) {
      throw new Error(`Couldn't read package.json of local dependency ${wantedDependency.alias ? wantedDependency.alias + '@' : ''}${wantedDependency.pref ?? ''}`)
    }
    return {
      body: {
        id,
        isLocal: true,
        manifest,
        normalizedPref,
        resolution: resolution as DirectoryResolution,
        resolvedVia,
        updated,
      },
    }
  }

  const isInstallable = (
    ctx.force === true ||
      (
        manifest == null
          ? undefined
          : packageIsInstallable(id, manifest, {
            engineStrict: ctx.engineStrict,
            lockfileDir: options.lockfileDir,
            nodeVersion: ctx.nodeVersion,
            optional: wantedDependency.optional === true,
            pnpmVersion: ctx.pnpmVersion,
          })
      )
  )
  // We can skip fetching the package only if the manifest
  // is present after resolution
  if ((options.skipFetch === true || isInstallable === false) && (manifest != null)) {
    return {
      body: {
        id,
        isLocal: false as const,
        isInstallable: isInstallable ?? undefined,
        latest,
        manifest,
        normalizedPref,
        resolution,
        resolvedVia,
        updated,
      },
    }
  }

  const fetchResult = ctx.fetchPackageToStore({
    fetchRawManifest: true,
    force: forceFetch,
    lockfileDir: options.lockfileDir,
    pkg: {
      ...pick(['name', 'version'], manifest ?? options.currentPkg ?? {}),
      id,
      resolution,
    },
  })

  return {
    body: {
      id,
      isLocal: false as const,
      isInstallable: isInstallable ?? undefined,
      latest,
      manifest,
      normalizedPref,
      resolution,
      resolvedVia,
      updated,
    },
    bundledManifest: fetchResult.bundledManifest,
    files: fetchResult.files,
    filesIndexFile: fetchResult.filesIndexFile,
    finishing: fetchResult.finishing,
  }
}

interface FetchLock {
  bundledManifest?: Promise<BundledManifest>
  files: Promise<PackageFilesResponse>
  filesIndexFile: string
  finishing: Promise<void>
}

function fetchToStore (
  ctx: {
    checkFilesIntegrity: (
      pkgIndex: Record<string, PackageFileInfo>,
      manifest?: DeferredManifestPromise
    ) => Promise<boolean>
    fetch: (
      packageId: string,
      resolution: Resolution,
      opts: FetchOptions
    ) => Promise<FetchResult>
    fetchingLocker: Map<string, FetchLock>
    getFilePathInCafs: (integrity: string, fileType: FileType) => string
    getFilePathByModeInCafs: (integrity: string, mode: number) => string
    requestsQueue: {add: <T>(fn: () => Promise<T>, opts: {priority: number}) => Promise<T>}
    storeDir: string
    verifyStoreIntegrity: boolean
  },
  opts: {
    pkg: {
      name?: string
      version?: string
      id: string
      resolution: Resolution
    }
    fetchRawManifest?: boolean
    force: boolean
    lockfileDir: string
  }
): {
    bundledManifest?: () => Promise<BundledManifest>
    filesIndexFile: string
    files: () => Promise<PackageFilesResponse>
    finishing: () => Promise<void>
  } {
  const targetRelative = depPathToFilename(opts.pkg.id, opts.lockfileDir)
  const target = path.join(ctx.storeDir, targetRelative)

  if (!ctx.fetchingLocker.has(opts.pkg.id)) {
    const bundledManifest = pDefer<BundledManifest>()
    const files = pDefer<PackageFilesResponse>()
    const finishing = pDefer<undefined>()
    const filesIndexFile = opts.pkg.resolution['integrity']
      ? ctx.getFilePathInCafs(opts.pkg.resolution['integrity'], 'index')
      : path.join(target, 'integrity.json')

    doFetchToStore(filesIndexFile, bundledManifest, files, finishing) // eslint-disable-line

    if (opts.fetchRawManifest) {
      ctx.fetchingLocker.set(opts.pkg.id, {
        bundledManifest: removeKeyOnFail(bundledManifest.promise),
        files: removeKeyOnFail(files.promise),
        filesIndexFile,
        finishing: removeKeyOnFail(finishing.promise),
      })
    } else {
      ctx.fetchingLocker.set(opts.pkg.id, {
        files: removeKeyOnFail(files.promise),
        filesIndexFile,
        finishing: removeKeyOnFail(finishing.promise),
      })
    }

    // When files resolves, the cached result has to set fromStore to true, without
    // affecting previous invocations: so we need to replace the cache.
    //
    // Changing the value of fromStore is needed for correct reporting of `pnpm server`.
    // Otherwise, if a package was not in store when the server started, it will be always
    // reported as "downloaded" instead of "reused".
    files.promise.then((cache) => { // eslint-disable-line
      progressLogger.debug({
        packageId: opts.pkg.id,
        requester: opts.lockfileDir,
        status: cache.fromStore
          ? 'found_in_store'
          : 'fetched',
      })

      // If it's already in the store, we don't need to update the cache
      if (cache.fromStore) {
        return
      }

      const tmp = ctx.fetchingLocker.get(opts.pkg.id)

      // If fetching failed then it was removed from the cache.
      // It is OK. In that case there is no need to update it.
      if (tmp == null) return

      ctx.fetchingLocker.set(opts.pkg.id, {
        ...tmp,
        files: Promise.resolve({
          ...cache,
          fromStore: true,
        }),
      })
    })
      .catch(() => {
        ctx.fetchingLocker.delete(opts.pkg.id)
      })
  }

  const result = ctx.fetchingLocker.get(opts.pkg.id)!

  if (opts.fetchRawManifest && (result.bundledManifest == null)) {
    result.bundledManifest = removeKeyOnFail(
      result.files.then(async (filesResult) => {
        if (!filesResult.local) {
          const { integrity, mode } = filesResult.filesIndex['package.json']
          const manifestPath = ctx.getFilePathByModeInCafs(integrity, mode)
          return readBundledManifest(manifestPath)
        }
        return readBundledManifest(filesResult.filesIndex['package.json'])
      })
    )
  }

  return {
    bundledManifest: (result.bundledManifest != null) ? pShare(result.bundledManifest) : undefined,
    files: pShare(result.files),
    filesIndexFile: result.filesIndexFile,
    finishing: pShare(result.finishing),
  }

  async function removeKeyOnFail<T> (p: Promise<T>): Promise<T> {
    try {
      return await p
    } catch (err: any) { // eslint-disable-line
      ctx.fetchingLocker.delete(opts.pkg.id)
      throw err
    }
  }

  async function doFetchToStore (
    filesIndexFile: string,
    bundledManifest: pDefer.DeferredPromise<BundledManifest>,
    files: pDefer.DeferredPromise<PackageFilesResponse>,
    finishing: pDefer.DeferredPromise<void>
  ) {
    try {
      const isLocalTarballDep = opts.pkg.id.startsWith('file:')
      const isLocalPkg = opts.pkg.resolution.type === 'directory'

      if (
        !opts.force &&
        (
          !isLocalTarballDep ||
          await tarballIsUpToDate(opts.pkg.resolution as any, target, opts.lockfileDir) // eslint-disable-line
        ) &&
        !isLocalPkg
      ) {
        let pkgFilesIndex
        try {
          pkgFilesIndex = await loadJsonFile<PackageFilesIndex>(filesIndexFile)
        } catch (err: any) { // eslint-disable-line
          // ignoring. It is fine if the integrity file is not present. Just refetch the package
        }
        // if target exists and it wasn't modified, then no need to refetch it

        if ((pkgFilesIndex?.files) != null) {
          const manifest = opts.fetchRawManifest
            ? safeDeferredPromise<DependencyManifest>()
            : undefined
          if (
            (
              pkgFilesIndex.name != null &&
              opts.pkg.name != null &&
              pkgFilesIndex.name.toLowerCase() !== opts.pkg.name.toLowerCase()
            ) ||
            (
              pkgFilesIndex.version != null &&
              opts.pkg.version != null &&
              // We used to not normalize the package versions before writing them to the lockfile and store.
              // So it may happen that the version will be in different formats.
              // For instance, v1.0.0 and 1.0.0
              // Hence, we need to use semver.eq() to compare them.
              !equalOrSemverEqual(pkgFilesIndex.version, opts.pkg.version)
            )
          ) {
            /* eslint-disable @typescript-eslint/restrict-template-expressions */
            throw new PnpmError('UNEXPECTED_PKG_CONTENT_IN_STORE', `\
Package name mismatch found while reading ${JSON.stringify(opts.pkg.resolution)} from the store. \
This means that the lockfile is broken. Expected package: ${opts.pkg.name}@${opts.pkg.version}. \
Actual package in the store by the given integrity: ${pkgFilesIndex.name}@${pkgFilesIndex.version}.`)
            /* eslint-enable @typescript-eslint/restrict-template-expressions */
          }
          const verified = await ctx.checkFilesIntegrity(pkgFilesIndex.files, manifest)
          if (verified) {
            files.resolve({
              filesIndex: pkgFilesIndex.files,
              fromStore: true,
              sideEffects: pkgFilesIndex.sideEffects,
            })
            if (manifest != null) {
              manifest()
                .then((manifest) => bundledManifest.resolve(normalizeBundledManifest(manifest)))
                .catch(bundledManifest.reject)
            }
            finishing.resolve(undefined)
            return
          }
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
      // As much tarballs should be downloaded simultaneously as possible.
      const priority = (++ctx.requestsQueue['counter'] % ctx.requestsQueue['concurrency'] === 0 ? -1 : 1) * 1000 // eslint-disable-line

      const fetchManifest = opts.fetchRawManifest
        ? safeDeferredPromise<DependencyManifest>()
        : undefined
      if (fetchManifest != null) {
        fetchManifest()
          .then((manifest) => bundledManifest.resolve(normalizeBundledManifest(manifest)))
          .catch(bundledManifest.reject)
      }
      const fetchedPackage = await ctx.requestsQueue.add(async () => ctx.fetch(
        opts.pkg.id,
        opts.pkg.resolution,
        {
          lockfileDir: opts.lockfileDir,
          manifest: fetchManifest,
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
        }
      ), { priority })

      let filesResult!: PackageFilesResponse
      if (!fetchedPackage.local) {
        const filesIndex = fetchedPackage.filesIndex
        // Ideally, files wouldn't care about when integrity is calculated.
        // However, we can only rename the temp folder once we know the package name.
        // And we cannot rename the temp folder till we're calculating integrities.
        const integrity: Record<string, PackageFileInfo> = {}
        await Promise.all(
          Object.keys(filesIndex)
            .map(async (filename) => {
              const {
                checkedAt,
                integrity: fileIntegrity,
              } = await filesIndex[filename].writeResult
              integrity[filename] = {
                checkedAt,
                integrity: fileIntegrity.toString(), // TODO: use the raw Integrity object
                mode: filesIndex[filename].mode,
                size: filesIndex[filename].size,
              }
            })
        )
        await writeJsonFile(filesIndexFile, {
          name: opts.pkg.name,
          version: opts.pkg.version,
          files: integrity,
        })
        filesResult = {
          fromStore: false,
          filesIndex: integrity,
        }
      } else {
        filesResult = {
          local: true,
          fromStore: false,
          filesIndex: fetchedPackage.filesIndex,
          packageImportMethod: fetchedPackage['packageImportMethod'],
        }
      }

      if (isLocalTarballDep && opts.pkg.resolution['integrity']) { // eslint-disable-line @typescript-eslint/dot-notation
        await fs.mkdir(target, { recursive: true })
        await gfs.writeFile(path.join(target, TARBALL_INTEGRITY_FILENAME), opts.pkg.resolution['integrity'], 'utf8') // eslint-disable-line @typescript-eslint/dot-notation
      }

      files.resolve(filesResult)
      finishing.resolve(undefined)
    } catch (err: any) { // eslint-disable-line
      files.reject(err)
      if (opts.fetchRawManifest) {
        bundledManifest.reject(err)
      }
    }
  }
}

async function writeJsonFile (filePath: string, data: Object) {
  const targetDir = path.dirname(filePath)
  // TODO: use the API of @pnpm/cafs to write this file
  // There is actually no need to create the directory in 99% of cases.
  // So by using cafs API, we'll improve performance.
  await fs.mkdir(targetDir, { recursive: true })
  const temp = pathTemp(targetDir)
  await gfs.writeFile(temp, JSON.stringify(data))
  await renameOverwrite(temp, filePath)
}

async function readBundledManifest (pkgJsonPath: string): Promise<BundledManifest> {
  return pickBundledManifest(await readPackage(pkgJsonPath) as DependencyManifest)
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
  fetcherByHostingType: {[hostingType: string]: FetchFunction},
  cafs: Cafs,
  packageId: string,
  resolution: Resolution,
  opts: FetchOptions
): Promise<FetchResult> {
  const fetch = fetcherByHostingType[resolution.type ?? 'tarball']
  if (!fetch) {
    throw new Error(`Fetching for dependency type "${resolution.type ?? 'undefined'}" is not supported`)
  }
  try {
    return await fetch(cafs, resolution, opts)
  } catch (err: any) { // eslint-disable-line
    packageRequestLogger.warn({
      message: `Fetching ${packageId} failed!`,
      prefix: opts.lockfileDir,
    })
    throw err
  }
}
