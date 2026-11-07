import type { Fetchers } from '@pnpm/fetching.fetcher-base'
import { type PickedFetcher, pickFetcher } from '@pnpm/fetching.pick-fetcher'
import type { CustomFetcher } from '@pnpm/hooks.types'
import type {
  AtomicResolution,
  DirectoryResolution,
  PreferredVersions,
  Resolution,
  ResolveFunction,
  ResolveResult,
  TarballResolution,
} from '@pnpm/resolving.resolver-base'
import type { Cafs } from '@pnpm/store.cafs-types'
import type {
  FetchPackageToStoreFunction,
  GetFilesIndexFilePath,
  PackageResponse,
  PkgNameVersion,
  PkgRequestFetchResult,
  RequestPackageFunction,
  RequestPackageOptions,
  WantedDependency,
} from '@pnpm/store.controller-types'
import type { DependencyManifest } from '@pnpm/types'
import {
  calcMaxWorkers,
  readPkgFromCafs as _readPkgFromCafs,
} from '@pnpm/worker'
import { loadJsonFile } from 'load-json-file'
import PQueue from 'p-queue'
import { pick } from 'ramda'

import { fetcher } from './fetcher.js'
import { fetchToStore } from './fetchToStore.js'
import { getFilesIndexFilePath } from './getFilesIndexFilePath.js'
import {
  applyReadPackageHook,
  checkInstallability,
  createInstallabilityCheck,
  type InstallabilityCheck,
  linkFileDepsInsidePackage,
} from './manifest.js'
import { getExpectedIntegrity } from './storeEntry.js'

export interface PackageRequesterOptions {
  engineStrict?: boolean
  force?: boolean
  forceIgnoresPlatform?: boolean
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
  frozenStore?: boolean
}

export function createPackageRequester (
  opts: PackageRequesterOptions
): RequestPackageFunction & {
  fetchPackageToStore: FetchPackageToStoreFunction
  getFilesIndexFilePath: GetFilesIndexFilePath
  requestPackage: RequestPackageFunction
} {
  opts = opts || {}

  // Downloads are I/O-bound, not CPU-bound: a low-latency registry only
  // saturates with enough requests in flight, so the floor matters more
  // than the per-core scaling — a CPU-derived floor left 4-core CI
  // runners draining multi-hundred-tarball installs 16 at a time.
  const networkConcurrency = opts.networkConcurrency ?? Math.min(96, Math.max(calcMaxWorkers() * 3, 64))
  const requestsQueue = new PQueue({
    concurrency: networkConcurrency,
  })

  const fetchPackageToStore = createFetchPackageToStore(opts, { requestsQueue, networkConcurrency })
  const requestPackage = resolveAndFetch.bind(null, {
    engineStrict: opts.engineStrict,
    nodeVersion: opts.nodeVersion,
    pnpmVersion: opts.pnpmVersion,
    force: opts.force,
    forceIgnoresPlatform: opts.forceIgnoresPlatform,
    fetchPackageToStore,
    requestsQueue,
    resolve: opts.resolve,
    storeDir: opts.storeDir,
    fetchers: opts.fetchers,
    customFetchers: opts.customFetchers,
  })

  return Object.assign(requestPackage, {
    fetchPackageToStore,
    getFilesIndexFilePath: getFilesIndexFilePath.bind(null, {
      storeDir: opts.storeDir,
      virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    }),
    requestPackage,
  })
}

function createFetchPackageToStore (
  opts: PackageRequesterOptions,
  { requestsQueue, networkConcurrency }: { requestsQueue: PQueue, networkConcurrency: number }
): FetchPackageToStoreFunction {
  const fetch = fetcher.bind(null, opts.fetchers, opts.cafs, opts.customFetchers)
  const readPkgFromCafs = _readPkgFromCafs.bind(null, {
    storeDir: opts.storeDir,
    verifyStoreIntegrity: opts.verifyStoreIntegrity,
    strictStorePkgContentCheck: opts.strictStorePkgContentCheck,
    frozenStore: opts.frozenStore,
  })
  return fetchToStore.bind(null, {
    readPkgFromCafs,
    fetch,
    fetchingLocker: new Map(),
    requestsQueue: Object.assign(requestsQueue, {
      counter: 0,
      concurrency: networkConcurrency,
    }),
    storeDir: opts.storeDir,
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    strictStorePkgContentCheck: opts.strictStorePkgContentCheck,
  })
}

interface ResolveAndFetchContext {
  engineStrict?: boolean
  force?: boolean
  forceIgnoresPlatform?: boolean
  nodeVersion?: string
  pnpmVersion?: string
  requestsQueue: { add: <Result>(fn: () => Promise<Result>, opts: { priority: number }) => Promise<Result> }
  resolve: ResolveFunction
  fetchPackageToStore: FetchPackageToStoreFunction
  storeDir: string
  fetchers: Fetchers
  customFetchers?: CustomFetcher[]
}

type RequestedDependency = WantedDependency & { optional?: boolean }

interface ResolvedRequest {
  ctx: ResolveAndFetchContext
  options: RequestPackageOptions
  resolveResult: ResolveResult
  id: ResolveResult['id']
  updated: boolean
  integrityChanged: boolean
  installability: InstallabilityCheck
  fetcherForResolution?: PickedFetcher
  resolutionNeedsFetch: boolean
}

interface ManifestState {
  manifest?: DependencyManifest
  hooked: boolean
  isInstallable: boolean | null | undefined
}

async function resolveAndFetch (
  ctx: ResolveAndFetchContext,
  wantedDependency: RequestedDependency,
  options: RequestPackageOptions
): Promise<PackageResponse> {
  const resolveResult = await resolveDependency(ctx, wantedDependency, options)
  const change = detectPackageChange(options.currentPkg, resolveResult)
  const { resolution } = resolveResult

  if (!change.updated) {
    carryOverIntegrity(resolution, change.previousIntegrity)
  }

  const id = resolveResult.id

  if ('type' in resolution && resolution.type === 'directory' && !id.startsWith('file:')) {
    return createLocalDirectoryResponse(resolveResult, { updated: change.updated, wantedDependency })
  }

  const prepared = await prepareResolvedManifest(resolveResult.manifest, resolution, options)
  const installability = createInstallabilityCheck({ ctx, id, options, wantedDependency })
  const isInstallable = installability.includeIncompatiblePackages ||
    (prepared.manifest == null ? undefined : checkInstallability(installability, prepared.manifest))
  const request: ResolvedRequest = {
    ctx,
    options,
    resolveResult,
    id,
    updated: change.updated,
    integrityChanged: change.integrityChanged,
    installability,
    ...await pickResolutionFetcher(ctx, resolution, id),
  }
  const state: ManifestState = { ...prepared, isInstallable }
  if (fetchCanBeSkipped(request, state)) {
    return { body: createPackageResponseBody(request, state) }
  }
  return fetchResolvedPackage(request, state)
}

async function resolveDependency (
  ctx: ResolveAndFetchContext,
  wantedDependency: RequestedDependency,
  options: RequestPackageOptions
): Promise<ResolveResult> {
  const { currentPkg } = options
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
  const preferredVersions: PreferredVersions = (currentPkg?.resolution && !options.update && currentPkg?.name != null && currentPkg?.version != null)
    ? {
      ...options.preferredVersions,
      [currentPkg.name]: { [currentPkg.version]: 'version' },
    }
    : options.preferredVersions

  return ctx.requestsQueue.add<ResolveResult>(async () => ctx.resolve(wantedDependency, {
    ...options,
    preferredVersions,
    currentPkg: (options.currentPkg?.id && options.currentPkg?.resolution)
      ? {
        id: options.currentPkg.id,
        name: options.currentPkg.name,
        version: options.currentPkg.version,
        resolution: options.currentPkg.resolution,
        publishedAt: options.currentPkg.publishedAt,
      }
      : undefined,
  }), { priority: options.downloadPriority })
}

function detectPackageChange (
  currentPkg: RequestPackageOptions['currentPkg'],
  resolveResult: ResolveResult
): { updated: boolean, integrityChanged: boolean, previousIntegrity: unknown } {
  // Use 'in' check to safely access integrity from any resolution type that has it
  const previousResolution = currentPkg?.resolution
  const previousIntegrity = previousResolution && 'integrity' in previousResolution ? previousResolution.integrity : undefined
  const newIntegrity = 'integrity' in resolveResult.resolution ? resolveResult.resolution.integrity : undefined
  const integrityChanged = previousIntegrity != null && newIntegrity != null && previousIntegrity !== newIntegrity

  const updated = currentPkg?.id !== resolveResult.id || !previousResolution || integrityChanged
  return { updated, integrityChanged, previousIntegrity }
}

// URL/tarball resolvers don't return an integrity, because it is only known
// after the tarball is downloaded. When a package is reused from the lockfile
// without being re-fetched, the freshly resolved resolution has no integrity,
// so carry it over from the current resolution instead of dropping it.
// https://github.com/pnpm/pnpm/issues/12001
function carryOverIntegrity (resolution: Resolution, previousIntegrity: unknown): void {
  if (
    typeof previousIntegrity === 'string' &&
    !resolution.type &&
    !(resolution as TarballResolution).integrity
  ) {
    (resolution as TarballResolution).integrity = previousIntegrity
  }
}

function createLocalDirectoryResponse (
  { id, manifest, resolution, resolvedVia, normalizedBareSpecifier, alias }: ResolveResult,
  { updated, wantedDependency }: { updated: boolean, wantedDependency: RequestedDependency }
): PackageResponse {
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

async function prepareResolvedManifest (
  resolvedManifest: DependencyManifest | undefined,
  resolution: Resolution,
  options: RequestPackageOptions
): Promise<Pick<ManifestState, 'manifest' | 'hooked'>> {
  let manifest = resolvedManifest
  if (manifest != null && resolution.type !== 'directory') {
    manifest = linkFileDepsInsidePackage(manifest)
  }
  if (options.readPackageHook == null || manifest == null) {
    return { manifest, hooked: false }
  }
  return { manifest: await applyReadPackageHook(options.readPackageHook, manifest), hooked: true }
}

async function pickResolutionFetcher (
  ctx: ResolveAndFetchContext,
  resolution: Resolution,
  id: string
): Promise<Pick<ResolvedRequest, 'fetcherForResolution' | 'resolutionNeedsFetch'>> {
  const fetcherForResolution = resolution.type === 'variations'
    ? undefined
    : await pickFetcher(ctx.fetchers, resolution as AtomicResolution, {
      customFetchers: ctx.customFetchers,
      packageId: id,
    })
  const resolutionNeedsFetchHook = fetcherForResolution?.resolutionNeedsFetch
  const resolutionNeedsFetch = typeof resolutionNeedsFetchHook === 'function'
    ? resolutionNeedsFetchHook(resolution)
    : false
  return { fetcherForResolution, resolutionNeedsFetch }
}

// Fetching can be skipped only when the manifest is available, the package content
// did not change, and the resolution does not need fetch-derived data.
function fetchCanBeSkipped (request: ResolvedRequest, state: ManifestState): boolean {
  return (request.options.skipFetch === true || state.isInstallable === false) &&
    !request.resolutionNeedsFetch && !request.integrityChanged && (state.manifest != null)
}

function createPackageResponseBody (request: ResolvedRequest, state: ManifestState): PackageResponse['body'] {
  const { resolveResult } = request
  return {
    id: request.id,
    isLocal: false as const,
    isInstallable: state.isInstallable ?? undefined,
    latest: resolveResult.latest,
    nonDeprecatedAlternative: resolveResult.nonDeprecatedAlternative,
    manifest: state.manifest,
    normalizedBareSpecifier: resolveResult.normalizedBareSpecifier,
    resolution: resolveResult.resolution,
    resolvedVia: resolveResult.resolvedVia,
    updated: request.updated,
    publishedAt: resolveResult.publishedAt,
    alias: resolveResult.alias,
    policyViolation: resolveResult.policyViolation,
    hooked: state.hooked,
  }
}

async function fetchResolvedPackage (request: ResolvedRequest, initialState: ManifestState): Promise<PackageResponse> {
  const { ctx, options, resolveResult: { resolution } } = request
  let state = initialState
  const fetchResult = ctx.fetchPackageToStore({
    allowBuild: options.allowBuild,
    fetchRawManifest: true,
    force: request.integrityChanged,
    populateMissingIntegrity: request.resolutionNeedsFetch,
    pickedFetcher: request.fetcherForResolution,
    ignoreScripts: options.ignoreScripts,
    lockfileDir: options.lockfileDir,
    pkg: {
      ...pickPkgNameVersion(request, state.manifest),
      id: request.id,
      resolution,
    },
    onFetchError: options.onFetchError,
    supportedArchitectures: options.supportedArchitectures,
  })

  if (!state.manifest) {
    const fetchedResult = await fetchResult.fetching()
    state = { ...state, manifest: (await readFetchedManifest(fetchedResult)) ?? state.manifest }
    // Add computed integrity to tarball resolutions. `variations` spans multiple
    // platform variants, so do not write one machine's integrity into the shared resolution.
    if (resolution.type !== 'variations') {
      addFetchedIntegrity(resolution, fetchedResult)
    }
  }

  const fetching = request.resolutionNeedsFetch
    ? populateIntegrityOnFetch(fetchResult.fetching, resolution)
    : fetchResult.fetching
  // Check installability now that we have the manifest (for git/tarball packages without registry metadata)
  if (state.isInstallable === undefined && state.manifest != null) {
    state = await checkFetchedManifestInstallability(request, { ...state, manifest: state.manifest })
  }
  return {
    body: createPackageResponseBody(request, state),
    fetching,
    filesIndexFile: fetchResult.filesIndexFile,
    resolutionNeedsFetch: request.resolutionNeedsFetch,
  }
}

function pickPkgNameVersion ({ options, updated }: ResolvedRequest, manifest: DependencyManifest | undefined): PkgNameVersion {
  const pkg: PkgNameVersion = manifest != null ? pick(['name', 'version'], manifest) : {}
  if (options.expectedPkg?.name == null) return pkg
  return updated ? { name: options.expectedPkg.name, version: pkg.version } : options.expectedPkg
}

async function readFetchedManifest (fetchedResult: PkgRequestFetchResult): Promise<DependencyManifest | undefined> {
  let manifest: DependencyManifest | undefined
  if (fetchedResult.bundledManifest) {
    manifest = fetchedResult.bundledManifest as DependencyManifest
  } else if (fetchedResult.files.filesMap.has('package.json')) {
    const loadedManifest = await loadJsonFile<Record<string, unknown>>(fetchedResult.files.filesMap.get('package.json')!)
    // Skip placeholder package.json added as a completion marker by the worker
    // for packages that genuinely lack one.
    if (!loadedManifest._pnpmPlaceholder) {
      manifest = loadedManifest as unknown as DependencyManifest
    }
  }
  return manifest != null ? linkFileDepsInsidePackage(manifest) : manifest
}

function addFetchedIntegrity (resolution: Resolution, fetchedResult: PkgRequestFetchResult): void {
  if (fetchedResult.integrity != null && getExpectedIntegrity(resolution) == null) {
    (resolution as TarballResolution).integrity = fetchedResult.integrity
  }
}

function populateIntegrityOnFetch (
  fetchPackage: () => Promise<PkgRequestFetchResult>,
  resolution: Resolution
): () => Promise<PkgRequestFetchResult> {
  let populating: Promise<PkgRequestFetchResult> | undefined
  return () => {
    populating ??= fetchPackage().then((fetchedResult) => {
      addFetchedIntegrity(resolution, fetchedResult)
      return fetchedResult
    }).catch((err: unknown) => {
      // Cache only fulfilled fetches; rejected fetches may be retried.
      populating = undefined
      throw err
    })
    return populating
  }
}

async function checkFetchedManifestInstallability (
  { installability, options }: ResolvedRequest,
  state: ManifestState & { manifest: DependencyManifest }
): Promise<ManifestState> {
  let { manifest, hooked } = state
  if (options.readPackageHook != null && !hooked) {
    manifest = await applyReadPackageHook(options.readPackageHook, manifest)
    hooked = true
  }
  return { manifest, hooked, isInstallable: checkInstallability(installability, manifest) }
}
