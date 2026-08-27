import { createPrivateKey } from 'node:crypto'
import fs from 'node:fs/promises'
import util from 'node:util'

import { calcDepState, calcDepStateInputKey, type DepsGraph, type DepsStateCache } from '@pnpm/deps.graph-hasher'
import type { ArtifactPins, LockfileObject, LockfileResolution } from '@pnpm/lockfile.types'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import type { PackageFilesResponse, StoreController, UploadPkgToStoreResult } from '@pnpm/store.controller-types'
import type { AllowBuild, DepPath, RegistryConfig, RemoteSideEffectsCacheSettings, SupportedArchitectures } from '@pnpm/types'
import pLimit from 'p-limit'

import {
  artifactBlobDigest,
  type ArtifactBlobUpload,
  type ArtifactCandidate,
  type ArtifactManifest,
  type ArtifactPayload,
  createSignedArtifactEnvelope,
  downloadSharedArtifactBlob,
  linuxGlibcCompatibilityTag,
  linuxGlibcSupportedTags,
  ownerNamespace,
  platformFingerprint,
  pnprSupportsSharedSideEffects,
  publishSharedSideEffects,
  resolveSharedSideEffects,
  type VerifiedArtifact,
} from './sharedSideEffects.js'

export interface RemoteSideEffectsInstallNode<T extends string> {
  graphKey: T
  depPath: DepPath
  files: PackageFilesResponse
  name: string
  patchFileHash?: string
  resolution: LockfileResolution
  version: string
}

export interface RemoteSideEffectsRestorerOptions<T extends string> {
  allowBuild?: AllowBuild
  artifactPinsLockfile?: LockfileObject
  configByUri: Record<string, RegistryConfig>
  depsGraph: DepsGraph<T>
  depsStateCache: DepsStateCache
  ignoreScripts: boolean
  nodeVersion?: string
  pnprServer?: string
  recordArtifactPins?: boolean
  settings?: RemoteSideEffectsCacheSettings
  sideEffectsCacheRead: boolean
  storeController: StoreController
  supportedArchitectures?: SupportedArchitectures
  onArtifactPinsChanged?: () => void
  warn?: (message: string) => void
}

export interface RemoteSideEffectsPrerequisites {
  ignoreScripts: boolean
  nodeVersion?: string
  pnprServer?: string
  settings?: RemoteSideEffectsCacheSettings
  storeController: StoreController
}

export interface RemoteSideEffectsRestorer<T extends string> {
  /**
   * Install pnpr's verified build of `node`, when it has one, into that node's
   * own `sideEffectsMaps` and return the key it was stored under. `undefined`
   * means the package has to be built locally, for any reason.
   *
   * Called once per package as its files arrive, so linking one package never
   * waits on an unrelated fetch. Calls raised close together still leave as a
   * single lookup request.
   */
  restore: (node: RemoteSideEffectsInstallNode<T>) => Promise<string | undefined>
}

/**
 * How long the first queued candidate waits for company before its lookup
 * leaves. Long enough to gather the packages whose fetches land together,
 * short enough that a lone candidate is not what holds up an install.
 */
const LOOKUP_BATCH_WINDOW = 20

/** Well under the protocol's candidate ceiling, so a batch is never refused. */
const MAX_LOOKUP_BATCH = 512

interface RestoredArtifact {
  added: Map<string, string>
  deleted: string[]
  envelopeDigest: string
}

interface QueuedLookup {
  candidate: ArtifactCandidate
  resolve: (artifact: RestoredArtifact | undefined) => void
}

export function canRestoreRemoteSideEffects (opts: RemoteSideEffectsPrerequisites): boolean {
  return opts.pnprServer != null &&
    opts.settings != null &&
    opts.settings.organization != null &&
    (opts.settings.packages?.length ?? 0) > 0 &&
    Object.keys(opts.settings.trustedKeys ?? {}).length > 0 &&
    !opts.ignoreScripts &&
    currentLinuxGlibcPlatform(opts.nodeVersion) != null &&
    opts.storeController.addFileToStore != null
}

export function createRemoteSideEffectsRestorer<T extends string> (
  opts: RemoteSideEffectsRestorerOptions<T>
): RemoteSideEffectsRestorer<T> | undefined {
  if (!canRestoreRemoteSideEffects(opts)) return undefined
  const platform = currentLinuxGlibcPlatform(opts.nodeVersion)
  const { pnprServer, settings } = opts
  const organization = settings?.organization
  if (platform == null || pnprServer == null || settings == null || organization == null) return undefined
  const registryUrl = pnprServer
  const ownerName = organization
  const owner = { type: 'organization', name: ownerName } as const
  let supportedTags: string[]
  try {
    supportedTags = linuxGlibcSupportedTags(platform)
  } catch (err: unknown) {
    opts.warn?.(`Remote side-effects platform is unsupported: ${errorMessage(err)}`)
    return undefined
  }
  const trustedKeys = settings.trustedKeys ?? {}
  const ownerKey = ownerNamespace(owner)
  const fingerprint = platformFingerprint(supportedTags)
  const pinnedEnvelopeDigests = new Map<string, string>()
  const pinCollisions = new Set<string>()
  const eligiblePackages = new Set(settings.packages)
  const authorization = createGetAuthHeaderByURI(opts.configByUri)(registryUrl)
  const artifactLimit = pLimit(4)
  const downloadLimit = pLimit(16)
  // A store probe reads and hashes the candidate, so it holds a descriptor for
  // as long as the file takes. A manifest may list `MAX_MANIFEST_FILES` paths
  // and several artifacts hydrate at once, so probing every file the moment it
  // is asked for would exhaust the descriptor table.
  const storeLookupLimit = pLimit(16)
  // Restorer-lifetime, so one blob shared by several artifacts is fetched and
  // stored once however the batches happen to fall.
  const storedBlobs = new Map<string, Promise<string>>()
  const identityByInputKey = new Map<string, string>()
  const collisions = new Set<string>()
  const lookups = new Map<string, Promise<RestoredArtifact | undefined>>()
  let queued: QueuedLookup[] = []
  let flushTimer: NodeJS.Timeout | undefined
  let supported: Promise<boolean> | undefined

  return { restore }

  async function restore (node: RemoteSideEffectsInstallNode<T>): Promise<string | undefined> {
    if (node.files.requiresBuild !== true || !eligiblePackages.has(node.name)) return undefined
    if (opts.allowBuild?.(node.depPath) !== true) return undefined
    const sourceIntegrity = verifiedIntegrity(node.resolution)
    if (sourceIntegrity == null) return undefined
    const inputKey = calcDepStateInputKey({
      depsGraph: opts.depsGraph,
      depPath: node.graphKey,
      patchFileHash: node.patchFileHash,
      supportedArchitectures: opts.supportedArchitectures,
    })
    const pinnedEnvelopeDigest = opts.artifactPinsLockfile?.packages?.[node.depPath]
      .artifactPins?.[inputKey]?.[ownerKey]?.[fingerprint]
    if (pinnedEnvelopeDigest != null) {
      const previous = pinnedEnvelopeDigests.get(inputKey)
      if (previous == null) {
        pinnedEnvelopeDigests.set(inputKey, pinnedEnvelopeDigest)
      } else if (previous !== pinnedEnvelopeDigest) {
        pinnedEnvelopeDigests.delete(inputKey)
        pinCollisions.add(inputKey)
      }
    }
    if (pinCollisions.has(inputKey)) {
      opts.warn?.(`Conflicting remote side-effects pins for ${node.name}@${node.version}; building locally`)
      return undefined
    }
    if (collisions.has(inputKey)) return undefined
    const identity = `${node.name}\0${node.version}\0${sourceIntegrity}`
    const knownIdentity = identityByInputKey.get(inputKey)
    if (knownIdentity == null) {
      identityByInputKey.set(inputKey, identity)
    } else if (knownIdentity !== identity) {
      // Two different packages hashing to one input key would make the cache
      // ambiguous. The signed payload is bound to a single package identity so
      // nothing incorrect can be restored, but stop trusting the key.
      opts.warn?.(`Remote side-effects input key collision for ${node.name}@${node.version}; building locally`)
      collisions.add(inputKey)
      lookups.delete(inputKey)
      return undefined
    }
    const localCacheKey = calcDepState(opts.depsGraph, opts.depsStateCache, node.graphKey, {
      includeDepGraphHash: true,
      patchFileHash: node.patchFileHash,
      supportedArchitectures: opts.supportedArchitectures,
      nodeVersion: opts.nodeVersion,
    })
    if (opts.sideEffectsCacheRead && node.files.sideEffectsMaps?.has(localCacheKey) === true) return undefined

    let lookup = lookups.get(inputKey)
    if (lookup == null) {
      lookup = enqueue({
        key: inputKey,
        package: { name: node.name, version: node.version },
        sourceIntegrity,
        owner,
      })
      lookups.set(inputKey, lookup)
    }
    const artifact = await lookup
    if (artifact == null) {
      if (pinnedEnvelopeDigests.has(inputKey)) {
        opts.warn?.(`Pinned remote side-effects artifact for ${node.name}@${node.version} is unavailable; building locally`)
      }
      return undefined
    }
    recordArtifactPin(node.depPath, inputKey, artifact.envelopeDigest)
    node.files.sideEffectsMaps ??= new Map()
    node.files.sideEffectsMaps.set(localCacheKey, { added: artifact.added, deleted: artifact.deleted })
    return localCacheKey
  }

  async function enqueue (candidate: ArtifactCandidate): Promise<RestoredArtifact | undefined> {
    let resolve!: (artifact: RestoredArtifact | undefined) => void
    const promise = new Promise<RestoredArtifact | undefined>((settle) => {
      resolve = settle
    })
    queued.push({ candidate, resolve })
    if (queued.length >= MAX_LOOKUP_BATCH) {
      flushNow()
    } else if (flushTimer == null) {
      flushTimer = setTimeout(flushNow, LOOKUP_BATCH_WINDOW)
      flushTimer.unref?.()
    }
    return promise
  }

  function flushNow (): void {
    if (flushTimer != null) {
      clearTimeout(flushTimer)
      flushTimer = undefined
    }
    const batch = queued
    queued = []
    if (batch.length > 0) void lookupBatch(batch)
  }

  async function lookupBatch (batch: QueuedLookup[]): Promise<void> {
    const eligibleBatch = batch.filter(({ candidate, resolve }) => {
      if (!collisions.has(candidate.key) && !pinCollisions.has(candidate.key)) return true
      resolve(undefined)
      return false
    })
    if (eligibleBatch.length === 0) return
    const batchPinnedEnvelopeDigests = new Map(pinnedEnvelopeDigests)
    supported ??= (async () => {
      try {
        return await pnprSupportsSharedSideEffects({ registryUrl, authorization })
      } catch (err: unknown) {
        opts.warn?.(`Remote side-effects cache handshake failed: ${errorMessage(err)}`)
        return false
      }
    })()
    if (!await supported) {
      for (const { resolve } of eligibleBatch) resolve(undefined)
      return
    }
    let resolved
    try {
      resolved = await resolveSharedSideEffects({
        registryUrl,
        authorization,
        candidates: eligibleBatch.map(({ candidate }) => candidate),
        supportedTags,
        policy: {
          ignoreScripts: false,
          eligiblePackages,
          allowedBuilds: new Set(eligibleBatch.map(({ candidate }) => candidate.package.name)),
        },
        trustedKeys,
        pinnedEnvelopeDigests: batchPinnedEnvelopeDigests,
      })
    } catch (err: unknown) {
      opts.warn?.(`Remote side-effects cache lookup failed: ${errorMessage(err)}`)
      for (const { resolve } of eligibleBatch) resolve(undefined)
      return
    }
    await Promise.all(eligibleBatch.map(async ({ candidate, resolve }) => {
      if (collisions.has(candidate.key) || pinCollisions.has(candidate.key)) {
        resolve(undefined)
        return
      }
      const artifact = resolved.get(candidate.key)
      if (artifact == null) {
        resolve(undefined)
        return
      }
      resolve(await artifactLimit(async () => hydrate(artifact, candidate)))
    }))
  }

  async function hydrate (
    artifact: VerifiedArtifact,
    candidate: ArtifactCandidate
  ): Promise<RestoredArtifact | undefined> {
    try {
      const added = new Map(await Promise.all(artifact.payload.manifest.added.map(async (file) => {
        const storedKey = `${file.integrity}\0${file.mode}`
        let stored = storedBlobs.get(storedKey)
        if (stored == null) {
          stored = (async () => {
            // A built package's files are mostly its own, and artifacts share
            // files with each other. The store addresses content by the digest
            // this manifest entry already carries, so anything it holds is the
            // same bytes and does not need transferring again.
            const present = await storeLookupLimit(async () => opts.storeController.locateFileInStore?.(
              artifactBlobDigest(file.integrity),
              file.mode
            ))
            if (present != null) return present
            const bytes = await downloadLimit(async () => downloadSharedArtifactBlob({
              registryUrl,
              authorization,
              request: {
                owner: artifact.payload.owner,
                integrity: file.integrity,
              },
            }))
            return opts.storeController.addFileToStore!(bytes, file.mode).filePath
          })()
          storedBlobs.set(storedKey, stored)
        }
        try {
          return [file.path, await stored] as const
        } catch (err: unknown) {
          if (storedBlobs.get(storedKey) === stored) storedBlobs.delete(storedKey)
          throw err
        }
      })))
      return { added, deleted: artifact.payload.manifest.deleted, envelopeDigest: artifact.envelopeDigest }
    } catch (err: unknown) {
      opts.warn?.(`Remote side-effects artifact for ${candidate.package.name}@${candidate.package.version} was rejected: ${errorMessage(err)}`)
      return undefined
    }
  }

  function recordArtifactPin (depPath: DepPath, inputKey: string, envelopeDigest: string): void {
    if (opts.recordArtifactPins !== true) return
    const snapshot = opts.artifactPinsLockfile?.packages?.[depPath]
    if (snapshot == null) return
    const previous = snapshot.artifactPins?.[inputKey]
    if (previous?.[ownerKey]?.[fingerprint] === envelopeDigest) return
    const artifactPins: ArtifactPins = {
      ...snapshot.artifactPins,
      [inputKey]: {
        ...previous,
        [ownerKey]: {
          ...previous?.[ownerKey],
          [fingerprint]: envelopeDigest,
        },
      },
    }
    snapshot.artifactPins = artifactPins
    pinnedEnvelopeDigests.set(inputKey, envelopeDigest)
    opts.onArtifactPinsChanged?.()
  }
}

export interface PublishBuiltSharedSideEffectsOptions<T extends string> {
  configByUri: Record<string, RegistryConfig>
  depsGraph: DepsGraph<T>
  graphKey: T
  name: string
  nodeVersion?: string
  patchFileHash?: string
  pnprServer?: string
  resolution: LockfileResolution
  settings?: RemoteSideEffectsCacheSettings
  supportedArchitectures?: SupportedArchitectures
  upload: UploadPkgToStoreResult
  version: string
}

export async function publishBuiltSharedSideEffects<T extends string> (
  opts: PublishBuiltSharedSideEffectsOptions<T>
): Promise<void> {
  if (
    opts.settings?.publish !== true ||
    opts.pnprServer == null ||
    opts.settings.packages?.includes(opts.name) !== true
  ) return
  const { builderId, keyId, organization, privateKey } = opts.settings
  if (organization == null) return
  const platform = currentLinuxGlibcPlatform(opts.nodeVersion)
  const sourceIntegrity = verifiedIntegrity(opts.resolution)
  if (keyId == null || privateKey == null || builderId == null || platform == null || sourceIntegrity == null) return
  const manifest = await artifactManifest(opts.upload)
  if (manifest == null) return
  const inputKey = calcDepStateInputKey({
    depsGraph: opts.depsGraph,
    depPath: opts.graphKey,
    patchFileHash: opts.patchFileHash,
    supportedArchitectures: opts.supportedArchitectures,
  })
  const payload: ArtifactPayload = {
    kind: 'dependency-side-effects:v1',
    package: { name: opts.name, version: opts.version },
    sourceIntegrity,
    inputKey,
    owner: { type: 'organization', name: organization },
    builderId,
    builderProfile: {
      imageDigest: opts.settings.imageDigest,
      architectureBaseline: opts.settings.architectureBaseline ?? process.arch,
      environment: opts.settings.buildEnv ?? {},
    },
    compatibility: {
      kind: 'tagged',
      tags: [linuxGlibcCompatibilityTag(platform)],
    },
    manifest: manifest.manifest,
  }
  await publishSharedSideEffects({
    registryUrl: opts.pnprServer,
    authorization: createGetAuthHeaderByURI(opts.configByUri)(opts.pnprServer),
    key: inputKey,
    envelope: createSignedArtifactEnvelope(payload, {
      keyId,
      privateKey: createPrivateKey({
        key: Buffer.from(privateKey, 'base64'),
        format: 'der',
        type: 'pkcs8',
      }),
    }),
    blobs: manifest.blobs,
  })
}

async function artifactManifest (upload: UploadPkgToStoreResult): Promise<{
  manifest: ArtifactManifest
  blobs: ArtifactBlobUpload[]
} | undefined> {
  const diff = upload.sideEffects
  if (diff == null) return undefined
  const entries = await Promise.all(Array.from(diff.added ?? [], async ([filePath, info]) => {
    const integrity = `sha512-${Buffer.from(info.digest, 'hex').toString('base64')}`
    const storedPath = upload.filesMap.get(filePath)
    if (storedPath == null) throw new Error(`Uploaded side-effects file ${JSON.stringify(filePath)} has no CAFS path`)
    const bytes = await fs.readFile(storedPath)
    return {
      blob: { integrity, data: bytes.toString('base64') },
      file: {
        path: filePath,
        integrity,
        mode: info.mode,
        size: info.size,
      },
    }
  }))
  const blobs = new Map(entries.map(({ blob }) => [blob.integrity, blob]))
  return {
    manifest: {
      added: entries.map(({ file }) => file),
      deleted: diff.deleted ?? [],
    },
    blobs: Array.from(blobs.values()),
  }
}

function currentLinuxGlibcPlatform (nodeVersion?: string): {
  architecture: string
  nodeMajor: number
  glibcMajor: number
  glibcMinor: number
} | undefined {
  if (process.platform !== 'linux' || !['x64', 'arm64'].includes(process.arch)) return undefined
  const report = process.report?.getReport() as { header?: { glibcVersionRuntime?: string } }
  const glibc = report.header?.glibcVersionRuntime?.split('.')
  const nodeMajor = Number((nodeVersion ?? process.version).replace(/^v/, '').split('.')[0])
  const glibcMajor = Number(glibc?.[0])
  const glibcMinor = Number(glibc?.[1])
  if (![nodeMajor, glibcMajor, glibcMinor].every(Number.isSafeInteger)) return undefined
  return {
    architecture: process.arch,
    nodeMajor,
    glibcMajor,
    glibcMinor,
  }
}

function verifiedIntegrity (resolution: LockfileResolution): string | undefined {
  const value = resolution as { type?: string, integrity?: unknown }
  return (value.type == null || value.type === 'binary') && typeof value.integrity === 'string'
    ? value.integrity
    : undefined
}

function errorMessage (err: unknown): string {
  return util.types.isNativeError(err) ? err.message : String(err)
}
