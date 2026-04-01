import util from 'node:util'

import { MetadataCache } from '@pnpm/cache.metadata'
import { PnpmError } from '@pnpm/error'
import { filterPkgMetadataByPublishDate } from '@pnpm/resolving.registry.pkg-metadata-filter'
import type { PackageInRegistry, PackageMeta, PackageMetaWithTime } from '@pnpm/resolving.registry.types'
import type { VersionSelectors } from '@pnpm/resolving.resolver-base'
import { pick } from 'ramda'
import semver from 'semver'

import type {
  ResolveMetadataDataMessage,
  ResolveMetadataDataResult,
  ResolveMetadataMessage,
  ResolveMetadataResult,
  ResolveMetadataSpec,
  SerializedPackageInRegistry,
  SerializedPackageMeta,
} from './types.js'

// ── MetadataCache pool (like storeIndexCache in start.ts) ───────────

const metadataCacheMap = new Map<string, MetadataCache>()

function getMetadataCache (cacheDir: string): MetadataCache {
  if (!metadataCacheMap.has(cacheDir)) {
    metadataCacheMap.set(cacheDir, new MetadataCache(cacheDir))
  }
  return metadataCacheMap.get(cacheDir)!
}

export function closeAllMetadataCaches (): void {
  for (const mc of metadataCacheMap.values()) {
    mc.close()
  }
  metadataCacheMap.clear()
}

// ── Helpers (moved from pickPackage.ts) ─────────────────────────────

const registryHostCache = new Map<string, string>()

function getRegistryHost (registry: string): string {
  let host = registryHostCache.get(registry)
  if (host == null) {
    host = new URL(registry).host
    registryHostCache.set(registry, host)
  }
  return host
}

function validatePackageName (pkgName: string): void {
  if (pkgName.includes('/') && pkgName[0] !== '@') {
    throw new PnpmError('INVALID_PACKAGE_NAME', `Package name ${pkgName} is invalid, it should have a @scope`)
  }
}

function loadMetaFromDb (db: MetadataCache, name: string, needsFull: boolean): PackageMeta | null {
  const row = db.get(name)
  if (!row) return null
  if (needsFull && !row.isFull) return null
  const meta = JSON.parse(row.data) as PackageMeta
  meta.cachedAt = row.cachedAt
  meta.etag = row.etag
  meta.modified = row.modified ?? meta.modified
  return meta
}

function isMissingTimeError (err: unknown): boolean {
  return (
    err != null &&
    typeof err === 'object' &&
    'code' in err &&
    (err as { code: string }).code === 'ERR_PNPM_MISSING_TIME'
  )
}

function clearMeta (pkg: PackageMeta): PackageMeta {
  const versions: PackageMeta['versions'] = {}
  for (const [version, info] of Object.entries(pkg.versions)) {
    versions[version] = pick([
      'name',
      'version',
      'bin',
      'directories',
      'devDependencies',
      'optionalDependencies',
      'dependencies',
      'peerDependencies',
      'dist',
      'engines',
      'peerDependenciesMeta',
      'cpu',
      'os',
      'libc',
      'deprecated',
      'bundleDependencies',
      'bundledDependencies',
      'hasInstallScript',
      '_npmUser',
    ], info)
  }

  return {
    name: pkg.name,
    'dist-tags': pkg['dist-tags'],
    versions,
    time: pkg.time,
    modified: pkg.modified,
  }
}

// ── pickPackageFromMeta logic (inlined from pickPackageFromMeta.ts) ──

interface PickPackageFromMetaOptions {
  preferredVersionSelectors: VersionSelectors | undefined
  publishedBy?: Date
  publishedByExclude?: (name: string) => boolean | string[]
}

interface RegistryPackageSpec {
  type: 'tag' | 'version' | 'range'
  name: string
  fetchSpec: string
}

function assertMetaHasTime (meta: PackageMeta): asserts meta is PackageMetaWithTime {
  if (meta.time == null) {
    throw new PnpmError('MISSING_TIME', `The metadata of ${meta.name} is missing the "time" field`)
  }
}

function parseModifiedDate (modified: string | undefined): Date | null {
  if (!modified) return null
  const date = new Date(modified)
  if (Number.isNaN(date.getTime())) return null
  return date
}

const semverRangeCache = new Map<string, semver.Range | null>()

function semverSatisfiesLoose (version: string, range: string): boolean {
  let semverRange = semverRangeCache.get(range)
  if (semverRange === undefined) {
    try {
      semverRange = new semver.Range(range, true)
    } catch {
      semverRange = null
    }
    semverRangeCache.set(range, semverRange)
  }

  if (semverRange) {
    try {
      return semverRange.test(new semver.SemVer(version, true))
    } catch {
      return false
    }
  }

  return false
}

interface PickVersionByVersionRangeOptions {
  meta: PackageMeta
  versionRange: string
  preferredVersionSelectors?: VersionSelectors
  publishedBy?: Date
}

type PickVersionByVersionRange = (options: PickVersionByVersionRangeOptions) => string | null

function pickLowestVersionByVersionRange (
  { meta, versionRange, preferredVersionSelectors }: PickVersionByVersionRangeOptions
): string | null {
  if (preferredVersionSelectors != null && Object.keys(preferredVersionSelectors).length > 0) {
    const prioritizedPreferredVersions = prioritizePreferredVersions(meta, versionRange, preferredVersionSelectors)
    for (const preferredVersions of prioritizedPreferredVersions) {
      const preferredVersion = semver.minSatisfying(preferredVersions, versionRange, true)
      if (preferredVersion) {
        return preferredVersion
      }
    }
  }
  if (versionRange === '*') {
    return Object.keys(meta.versions).sort(semver.compare)[0]
  }
  return semver.minSatisfying(Object.keys(meta.versions), versionRange, true)
}

function pickVersionByVersionRange ({ meta, versionRange, preferredVersionSelectors }: PickVersionByVersionRangeOptions): string | null {
  const latest: string | undefined = meta['dist-tags'].latest

  if (preferredVersionSelectors != null && Object.keys(preferredVersionSelectors).length > 0) {
    const prioritizedPreferredVersions = prioritizePreferredVersions(meta, versionRange, preferredVersionSelectors)
    for (const preferredVersions of prioritizedPreferredVersions) {
      if (preferredVersions.includes(latest) && semverSatisfiesLoose(latest, versionRange)) {
        return latest
      }
      const preferredVersion = semver.maxSatisfying(preferredVersions, versionRange, true)
      if (preferredVersion) {
        return preferredVersion
      }
    }
  }

  const versions = Object.keys(meta.versions)
  if (latest && (versionRange === '*' || semverSatisfiesLoose(latest, versionRange))) {
    return latest
  }

  const maxVersion = semver.maxSatisfying(versions, versionRange, true)

  if (maxVersion && meta.versions[maxVersion].deprecated && versions.length > 1) {
    const nonDeprecatedVersions = versions.map((version) => meta.versions[version])
      .filter((versionMeta) => !versionMeta.deprecated)
      .map((versionMeta) => versionMeta.version)

    const maxNonDeprecatedVersion = semver.maxSatisfying(nonDeprecatedVersions, versionRange, true)
    if (maxNonDeprecatedVersion) return maxNonDeprecatedVersion
  }
  return maxVersion
}

function prioritizePreferredVersions (
  meta: PackageMeta,
  versionRange: string,
  preferredVerSelectors?: VersionSelectors
): string[][] {
  const preferredVerSelectorsArr = Object.entries(preferredVerSelectors ?? {})
  const versionsPrioritizer = new PreferredVersionsPrioritizer()

  for (const version of Object.keys(meta.versions)) {
    if (semverSatisfiesLoose(version, versionRange)) {
      versionsPrioritizer.add(version, 0)
    }
  }

  for (const [preferredSelector, preferredSelectorType] of preferredVerSelectorsArr) {
    const { selectorType, weight } = typeof preferredSelectorType === 'string'
      ? { selectorType: preferredSelectorType, weight: 1 }
      : preferredSelectorType
    if (preferredSelector === versionRange) continue
    switch (selectorType) {
      case 'tag': {
        versionsPrioritizer.add(meta['dist-tags'][preferredSelector], weight)
        break
      }
      case 'range': {
        const versions = Object.keys(meta.versions)
        for (const version of versions) {
          if (semverSatisfiesLoose(version, preferredSelector)) {
            versionsPrioritizer.add(version, weight)
          }
        }
        break
      }
      case 'version': {
        if (meta.versions[preferredSelector]) {
          versionsPrioritizer.add(preferredSelector, weight)
        }
        break
      }
    }
  }
  return versionsPrioritizer.versionsByPriority()
}

class PreferredVersionsPrioritizer {
  private preferredVersions: Record<string, number> = {}

  add (version: string, weight: number): void {
    if (!this.preferredVersions[version]) {
      this.preferredVersions[version] = weight
    } else {
      this.preferredVersions[version] += weight
    }
  }

  versionsByPriority (): string[][] {
    const versionsByWeight = Object.entries(this.preferredVersions)
      .reduce((acc, [version, weight]) => {
        acc[weight] = acc[weight] ?? []
        acc[weight].push(version)
        return acc
      }, {} as Record<number, string[]>)
    return Object.keys(versionsByWeight)
      .sort((a, b) => parseInt(b, 10) - parseInt(a, 10))
      .map((weight) => versionsByWeight[parseInt(weight, 10)])
  }
}

function pickPackageFromMeta (
  pickVersionByVersionRangeFn: PickVersionByVersionRange,
  {
    preferredVersionSelectors,
    publishedBy,
    publishedByExclude,
  }: PickPackageFromMetaOptions,
  spec: RegistryPackageSpec,
  meta: PackageMeta
): PackageInRegistry | null {
  if (publishedBy) {
    const excludeResult = publishedByExclude?.(meta.name) ?? false
    if (excludeResult !== true) {
      if (meta.time != null) {
        assertMetaHasTime(meta)
        const trustedVersions = Array.isArray(excludeResult) ? excludeResult : undefined
        meta = filterPkgMetadataByPublishDate(meta, publishedBy, trustedVersions)
      } else {
        const modifiedDate = parseModifiedDate(meta.modified)
        if (modifiedDate == null || modifiedDate >= publishedBy) {
          assertMetaHasTime(meta)
        }
      }
    }
  }
  if ((!meta.versions || Object.keys(meta.versions).length === 0) && !publishedBy) {
    if (meta.time?.unpublished?.versions?.length) {
      throw new PnpmError('UNPUBLISHED_PKG', `No versions available for ${spec.name} because it was unpublished`)
    }
    throw new PnpmError('NO_VERSIONS', `No versions available for ${spec.name}. The package may be unpublished.`)
  }
  try {
    let version!: string | null
    switch (spec.type) {
      case 'version':
        version = spec.fetchSpec
        break
      case 'tag':
        version = meta['dist-tags'][spec.fetchSpec]
        break
      case 'range':
        version = pickVersionByVersionRangeFn({
          meta,
          versionRange: spec.fetchSpec,
          preferredVersionSelectors,
          publishedBy,
        })
        break
    }
    if (!version) return null
    const manifest = meta.versions[version]
    if (manifest && meta['name']) {
      manifest.name = meta['name']
    }
    return manifest
  } catch (err: unknown) {
    if (
      util.types.isNativeError(err) &&
      'code' in err &&
      typeof err.code === 'string' &&
      err.code.startsWith('ERR_PNPM_')
    ) {
      throw err
    }
    throw new PnpmError('MALFORMED_METADATA',
      `Received malformed metadata for "${spec.name}"`,
      { hint: 'This might mean that the package was unpublished from the registry', cause: err }
    )
  }
}

// ── Build version-picker closure ────────────────────────────────────

function buildPickPackageFromMeta (msg: {
  pickLowestVersion?: boolean
  updateToLatest?: boolean
  publishedBy?: number
  publishedByExcludeResult?: boolean | string[]
  strictPublishedByCheck?: boolean
  preferredVersionSelectors?: VersionSelectors
  spec: ResolveMetadataSpec
}): (meta: PackageMeta) => PackageInRegistry | null {
  const publishedBy = msg.publishedBy != null ? new Date(msg.publishedBy) : undefined
  const publishedByExclude = msg.publishedByExcludeResult != null
    ? () => msg.publishedByExcludeResult!
    : undefined

  const pickPackageFromMetaUsingTimeStrict = pickPackageFromMeta.bind(null, pickVersionByVersionRange)

  function pickPackageFromMetaUsingTime (
    opts: PickPackageFromMetaOptions,
    spec: RegistryPackageSpec,
    meta: PackageMeta
  ): PackageInRegistry | null {
    const pickedPackage = pickPackageFromMeta(pickVersionByVersionRange, opts, spec, meta)
    if (pickedPackage) return pickedPackage
    return pickPackageFromMeta(pickLowestVersionByVersionRange, {
      preferredVersionSelectors: opts.preferredVersionSelectors,
    }, spec, meta)
  }

  const pickPackageFromMetaBySpec = (
    publishedBy
      ? (msg.strictPublishedByCheck ? pickPackageFromMetaUsingTimeStrict : pickPackageFromMetaUsingTime)
      : (pickPackageFromMeta.bind(null, msg.pickLowestVersion ? pickLowestVersionByVersionRange : pickVersionByVersionRange))
  ).bind(null, {
    preferredVersionSelectors: msg.preferredVersionSelectors,
    publishedBy,
    publishedByExclude,
  })

  const spec = msg.spec as RegistryPackageSpec

  if (msg.updateToLatest) {
    return (meta) => {
      const latestStableSpec: RegistryPackageSpec = { ...spec, type: 'tag', fetchSpec: 'latest' }
      const latestStable = pickPackageFromMetaBySpec(latestStableSpec, meta)
      const current = pickPackageFromMetaBySpec(spec, meta)

      if (!latestStable) return current
      if (!current) return latestStable
      if (semver.lt(latestStable.version, current.version)) return current
      return latestStable
    }
  }
  return pickPackageFromMetaBySpec.bind(null, spec)
}

// ── Serializers ─────────────────────────────────────────────────────

function serializeMeta (meta: PackageMeta): SerializedPackageMeta {
  return meta as unknown as SerializedPackageMeta
}

function serializePackage (pkg: PackageInRegistry | null): SerializedPackageInRegistry | null {
  if (pkg == null) return null
  return pkg as unknown as SerializedPackageInRegistry
}

// ── Handlers ────────────────────────────────────────────────────────

export function handleResolveMetadata (msg: ResolveMetadataMessage): ResolveMetadataResult {
  validatePackageName(msg.spec.name)

  const db = getMetadataCache(msg.cacheDir)
  const registryName = getRegistryHost(msg.registry)
  const dbName = `${registryName}/${msg.spec.name}`
  const fullMetadata = msg.fullMetadata === true
  const _pickPackageFromMeta = buildPickPackageFromMeta(msg)

  let metaCachedInStore: PackageMeta | null | undefined

  if (msg.offline === true || msg.preferOffline === true || msg.pickLowestVersion) {
    metaCachedInStore = loadMetaFromDb(db, dbName, fullMetadata)

    if (msg.offline) {
      if (metaCachedInStore != null) return {
        status: 'success',
        cacheHit: true,
        pickedPackage: serializePackage(_pickPackageFromMeta(metaCachedInStore)),
        meta: serializeMeta(metaCachedInStore),
      }
      throw new PnpmError('NO_OFFLINE_META', `Failed to resolve ${msg.spec.name}@${msg.spec.fetchSpec} in package mirror for ${msg.spec.name}`)
    }

    if (metaCachedInStore != null) {
      const pickedPackage = _pickPackageFromMeta(metaCachedInStore)
      if (pickedPackage) {
        return {
          status: 'success',
          cacheHit: true,
          pickedPackage: serializePackage(pickedPackage),
          meta: serializeMeta(metaCachedInStore),
        }
      }
    }
  }

  if (!msg.updateToLatest && msg.spec.type === 'version') {
    metaCachedInStore = metaCachedInStore ?? loadMetaFromDb(db, dbName, fullMetadata)
    if ((metaCachedInStore?.versions?.[msg.spec.fetchSpec]) != null) {
      try {
        const pickedPackage = _pickPackageFromMeta(metaCachedInStore)
        if (pickedPackage) {
          return {
            status: 'success',
            cacheHit: true,
            pickedPackage: serializePackage(pickedPackage),
            meta: serializeMeta(metaCachedInStore),
          }
        }
      } catch (err) {
        if (msg.strictPublishedByCheck) throw err
      }
    }
  }

  if (msg.publishedBy != null) {
    metaCachedInStore = metaCachedInStore ?? loadMetaFromDb(db, dbName, fullMetadata)
    const publishedByDate = new Date(msg.publishedBy)
    if (metaCachedInStore?.cachedAt && new Date(metaCachedInStore.cachedAt) >= publishedByDate) {
      try {
        const pickedPackage = _pickPackageFromMeta(metaCachedInStore)
        if (pickedPackage) {
          return {
            status: 'success',
            cacheHit: true,
            pickedPackage: serializePackage(pickedPackage),
            meta: serializeMeta(metaCachedInStore),
          }
        }
      } catch (err: unknown) {
        if (msg.strictPublishedByCheck && !isMissingTimeError(err)) throw err
      }
    }
  }

  // Cache miss - need to fetch from registry
  let etag = metaCachedInStore?.etag
  let modified = metaCachedInStore?.modified ?? metaCachedInStore?.time?.modified
  if (!etag || !modified) {
    const headers = db.getHeaders(dbName)
    etag = etag ?? headers?.etag
    modified = modified ?? headers?.modified
  }

  return {
    status: 'success',
    cacheHit: false,
    needsFetch: true,
    etag,
    modified,
  }
}

export function handleResolveMetadataData (msg: ResolveMetadataDataMessage): ResolveMetadataDataResult {
  const db = getMetadataCache(msg.cacheDir)
  const registryName = getRegistryHost(msg.registry)
  const dbName = `${registryName}/${msg.spec.name}`
  const fullMetadata = msg.fullMetadata === true
  const _pickPackageFromMeta = buildPickPackageFromMeta(msg)
  const publishedByDate = msg.publishedBy != null ? new Date(msg.publishedBy) : undefined
  const publishedByExcludeResult = msg.publishedByExcludeResult

  // 304 Not Modified -- re-read from cache and update cachedAt
  if (msg.notModified) {
    const metaCachedInStore = loadMetaFromDb(db, dbName, false)
    if (metaCachedInStore != null) {
      const cachedAt = Date.now()
      metaCachedInStore.cachedAt = cachedAt
      db.updateCachedAt(dbName, cachedAt)
      return {
        status: 'success',
        pickedPackage: serializePackage(_pickPackageFromMeta(metaCachedInStore)),
        meta: serializeMeta(metaCachedInStore),
      }
    }
    throw new PnpmError('CACHE_MISSING_AFTER_304',
      `Metadata cache for ${msg.spec.name} is unreadable after receiving 304 Not Modified`)
  }

  const cachedAt = Date.now()
  let meta = JSON.parse(msg.jsonText) as PackageMeta

  // When minimumReleaseAge is active and we fetched abbreviated metadata,
  // check if the package was recently modified and needs full metadata.
  if (
    publishedByDate &&
    !fullMetadata &&
    meta.time == null &&
    publishedByExcludeResult !== true
  ) {
    const modifiedDate = meta.modified ? new Date(meta.modified) : null
    const isModifiedValid = modifiedDate != null && !Number.isNaN(modifiedDate.getTime())
    if (!isModifiedValid || modifiedDate >= publishedByDate) {
      // Save abbreviated metadata before re-fetching full
      if (!msg.dryRun) {
        db.queueSet(dbName, msg.jsonText, {
          etag: msg.etag,
          modified: meta.modified,
          cachedAt,
        })
      }
      return {
        status: 'success',
        needsFullRefetch: true,
        meta: serializeMeta(meta),
      }
    }
  }

  if (msg.filterMetadata) {
    meta = clearMeta(meta)
  }
  meta.cachedAt = cachedAt
  meta.etag = msg.etag
  if (!msg.dryRun) {
    const rawJson = (msg.filterMetadata || typeof msg.jsonText !== 'string')
      ? JSON.stringify(meta)
      : msg.jsonText
    db.queueSet(dbName, rawJson, {
      etag: msg.etag,
      modified: meta.modified ?? meta.time?.modified,
      cachedAt,
      isFull: fullMetadata,
    })
  }
  return {
    status: 'success',
    pickedPackage: serializePackage(_pickPackageFromMeta(meta)),
    meta: serializeMeta(meta),
  }
}
