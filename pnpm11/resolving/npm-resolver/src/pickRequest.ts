import type { PackageInRegistry, PackageMeta } from '@pnpm/resolving.registry.types'
import type { TrustPolicy } from '@pnpm/types'
import type { LimitFunction } from 'p-limit'

import { retainsFullMeta } from './clearMeta.js'
import { condenseMetaForCache, loadMeta, type MetaHeaders } from './metaMirror.js'
import type { RegistryPackageSpec } from './parseBareSpecifier.js'
import type { PeekManifestFromStore, PickerOptions } from './pickMatchingVersion.js'
import type { PickPackageFromMetaOptions } from './pickPackageFromMeta.js'
import type { FetchPackageMeta } from './releaseAgeUpgrade.js'

export interface PackageMetaCache {
  /**
   * Must return the same object reference that `set` stored for the key: the
   * resolver tracks whether a cached packument was validated against the
   * registry by object identity (see `unverifiedDiskPackuments`). In a cache
   * that clones or deserializes on read, that provenance is lost and recovery
   * degrades — a stale disk-promoted entry that can't satisfy a spec fails
   * the pick instead of falling through to the registry.
   */
  get: (key: string) => PackageMeta | undefined
  set: (key: string, meta: PackageMeta) => void
  has: (key: string) => boolean
}

export interface PickPackageOptions extends PickPackageFromMetaOptions {
  fallbackPublishedBy?: Date
  authHeaderValue?: string
  pickLowestVersion?: boolean
  registry: string
  dryRun: boolean
  includeLatestTag?: boolean
  optional?: boolean
  trustPolicy?: TrustPolicy
  /**
   * When true, force a conditional registry request so a stale on-disk
   * packument can't satisfy the call: the on-disk exact-version fast
   * path is skipped, and the in-memory cache is bypassed too.
   */
  updateChecksums?: boolean
  /**
   * `pnpm update` must see versions published since the mirror was
   * written, so it does not reuse an ETag-less mirror.
   */
  refreshMetadata?: boolean
}

export interface PickPackageContext {
  fetch: FetchPackageMeta
  fullMetadata?: boolean
  /**
   * Whether a time-based resolution has to read the full metadata of
   * `registry`, because that registry's abbreviated form carries no `time`.
   * Asked per registry: the answer differs between the public registry and a
   * proxy that does carry it, and the mirror path and cache key below are
   * already keyed by registry, so two registries may disagree within one
   * install.
   */
  needsFullMetadataFor?: (registry: string) => boolean
  metaCache: PackageMetaCache
  cacheDir: string
  offline?: boolean
  preferOffline?: boolean
  peekManifestFromStore?: PeekManifestFromStore
  filterMetadata?: boolean
  ignoreMissingTimeField?: boolean
  /** Packuments whose release-age upgrade fetch already answered 304 in this resolver. */
  releaseAgeUpgradeCheckedPackuments?: WeakSet<PackageMeta>
}

export interface PickResult {
  meta: PackageMeta
  pickedPackage: PackageInRegistry | null
}

/** Everything one `pickPackage` call derives from its arguments. */
export interface PickRequest {
  ctx: PickPackageContext
  spec: RegistryPackageSpec
  opts: PickPackageOptions
  pickerOpts: PickerOptions
  fullMetadata: boolean
  metaDir: string
  cacheKey: string
  pkgMirror: string
}

/**
 * A pick running inside the mirror's limiter. The mirror is read at most once
 * per pick, so the steps share what they already read.
 */
export interface MirrorSession {
  request: PickRequest
  limit: LimitFunction
  diskMeta?: PackageMeta | null
  /**
   * Undefined until the headers are read, so the conditional request does
   * not read them a second time.
   */
  mirrorHeaders?: MetaHeaders | null
}

export async function loadMetaCondensed (request: PickRequest): Promise<PackageMeta | null> {
  const meta = await loadMeta(request.pkgMirror, { condense: !retainsFullMeta(request.ctx) })
  return meta == null ? null : condenseMetaForCache(request.ctx, meta)
}

export async function loadSessionDiskMeta (session: MirrorSession): Promise<PackageMeta | null> {
  session.diskMeta = session.diskMeta ?? await session.limit(async () => loadMetaCondensed(session.request))
  return session.diskMeta
}
