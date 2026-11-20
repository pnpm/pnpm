import { createPrivateKey } from 'node:crypto'
import fs from 'node:fs/promises'

import { calcDepState, calcDepStateInputKey, type DepsGraph } from '@pnpm/deps.graph-hasher'
import type { LockfileResolution } from '@pnpm/lockfile.types'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import type { StoreController, UploadPkgToStoreResult } from '@pnpm/store.controller-types'
import type { RegistryConfig, RemoteSideEffectsCacheSettings, SupportedArchitectures } from '@pnpm/types'
import pLimit from 'p-limit'

import { artifactCompatibilityTag, type ArtifactPlatform, artifactSupportedTags, currentArtifactPlatform } from './artifactPlatform.js'
import { errorMessage } from './errorMessage.js'
import { hydrate } from './hydrateSharedArtifact.js'
import { lookupArtifact } from './remoteSideEffectsLookup.js'
import {
  adoptStoredQuarantine,
  type RemoteSideEffectsInstallNode,
  type RemoteSideEffectsRestorerOptions,
  type RestoredArtifact,
  type RestorerContext,
  trackFilesIndexFile,
} from './remoteSideEffectsRestorerContext.js'
import {
  type ArtifactBlobUpload,
  type ArtifactManifest,
  type ArtifactPayload,
  createSignedArtifactEnvelope,
  type DependencySideEffectsCandidate,
  publishSharedSideEffects,
} from './sharedSideEffects.js'
import { storedArtifactIsVerified } from './storedArtifactVerification.js'

export type { RemoteSideEffectsInstallNode, RemoteSideEffectsRestorerOptions } from './remoteSideEffectsRestorerContext.js'

export interface RemoteSideEffectsPrerequisites {
  ignoreScripts: boolean
  nodeVersion?: string
  pnprServer?: string
  settings?: RemoteSideEffectsCacheSettings
  storeController: StoreController
}

export interface RemoteSideEffectsRestorer<GraphKey extends string> {
  /**
   * Install pnpr's verified build of `node`, when it has one, into that node's
   * own `sideEffectsMaps` and return the key it was stored under. `undefined`
   * means the package has to be built locally, for any reason.
   *
   * Called once per package as its files arrive, so linking one package never
   * waits on an unrelated fetch. Calls raised close together still leave as a
   * single lookup request.
   */
  restore: (node: RemoteSideEffectsInstallNode<GraphKey>) => Promise<string | undefined>
}

interface RestoreTarget<GraphKey extends string> {
  node: RemoteSideEffectsInstallNode<GraphKey>
  candidate: DependencySideEffectsCandidate
  localCacheKey: string
}

interface PublicationInputs {
  pnprServer: string
  settings: RemoteSideEffectsCacheSettings
  organization: string
  builderId: string
  keyId: string
  privateKey: string
  artifactPlatform: ArtifactPlatform
  sourceIntegrity: string
}

export function canRestoreRemoteSideEffects (opts: RemoteSideEffectsPrerequisites): boolean {
  return opts.settings != null &&
    isNonEmpty(opts.settings.org) &&
    (opts.settings.packages?.length ?? 0) > 0 &&
    Object.keys(opts.settings.trustedKeys ?? {}).length > 0 &&
    !opts.ignoreScripts &&
    currentArtifactPlatform(opts.nodeVersion) != null
}


export function createRemoteSideEffectsRestorer<GraphKey extends string> (
  opts: RemoteSideEffectsRestorerOptions<GraphKey>
): RemoteSideEffectsRestorer<GraphKey> | undefined {
  if (!canRestoreRemoteSideEffects(opts)) return undefined
  const artifactPlatform = currentArtifactPlatform(opts.nodeVersion)
  const { settings } = opts
  const organization = settings?.org
  if (artifactPlatform == null || settings == null || !isNonEmpty(organization)) return undefined
  let supportedTags: string[]
  try {
    supportedTags = artifactSupportedTags(artifactPlatform)
  } catch (err: unknown) {
    opts.warn?.(`Remote side-effects platform is unsupported: ${errorMessage(err)}`)
    return undefined
  }
  const ctx = createRestorerContext(opts, { settings, organization, supportedTags })
  return {
    restore: async (node) => restore(ctx, node),
  }
}

function createRestorerContext<GraphKey extends string> (
  opts: RemoteSideEffectsRestorerOptions<GraphKey>,
  { settings, organization, supportedTags }: {
    settings: RemoteSideEffectsCacheSettings
    organization: string
    supportedTags: string[]
  }
): RestorerContext<GraphKey> {
  const registryUrl = opts.pnprServer
  return {
    opts,
    registryUrl,
    owner: { type: 'organization', name: organization } as const,
    supportedTags,
    trustedKeys: settings.trustedKeys ?? {},
    eligiblePackages: new Set(settings.packages),
    authorization: registryUrl == null ? undefined : createGetAuthHeaderByURI(opts.configByUri)(registryUrl),
    artifactLimit: pLimit(4),
    downloadLimit: pLimit(16),
    storeLookupLimit: pLimit(16),
    storedBlobs: new Map(),
    identityByInputKey: new Map(),
    collisions: new Set(),
    lookups: new Map(),
    filesIndexFilesByInputKey: new Map(),
    quarantinedEnvelopeDigests: new Map(),
    lookupQueue: { queued: [] },
  }
}

async function restore<GraphKey extends string> (
  ctx: RestorerContext<GraphKey>,
  node: RemoteSideEffectsInstallNode<GraphKey>
): Promise<string | undefined> {
  const candidate = identifyCandidate(ctx, node)
  if (candidate == null) return undefined
  const target: RestoreTarget<GraphKey> = {
    node,
    candidate,
    localCacheKey: calcDepState(ctx.opts.depsGraph, ctx.opts.depsStateCache, node.graphKey, {
      includeDepGraphHash: true,
      patchFileHash: node.patchFileHash,
      supportedArchitectures: ctx.opts.supportedArchitectures,
      nodeVersion: ctx.opts.nodeVersion,
    }),
  }
  const localOutcome = await reuseLocalSideEffects(ctx, target)
  if (localOutcome != null) return localOutcome.cacheKey
  if (ctx.registryUrl == null || ctx.opts.storeController.addFileToStore == null) return undefined
  return restoreFromRegistry(ctx, target, ctx.registryUrl)
}

function identifyCandidate<GraphKey extends string> (
  ctx: RestorerContext<GraphKey>,
  node: RemoteSideEffectsInstallNode<GraphKey>
): DependencySideEffectsCandidate | undefined {
  if (node.files.requiresBuild !== true || !ctx.eligiblePackages.has(node.name)) return undefined
  if (ctx.opts.allowBuild?.(node.depPath) !== true) return undefined
  const sourceIntegrity = verifiedIntegrity(node.resolution)
  if (sourceIntegrity == null) return undefined
  const inputKey = calcDepStateInputKey({
    depsGraph: ctx.opts.depsGraph,
    depPath: node.graphKey,
    patchFileHash: node.patchFileHash,
    supportedArchitectures: ctx.opts.supportedArchitectures,
  })
  if (!claimInputKey(ctx, node, { inputKey, identity: `${node.name}\0${node.version}\0${sourceIntegrity}` })) return undefined
  return {
    key: inputKey,
    subject: {
      kind: 'dependency-side-effects',
      package: { name: node.name, version: node.version },
      sourceIntegrity,
    },
    owner: ctx.owner,
  }
}

/**
 * Bind `inputKey` to the package identity first seen with it. Returns false
 * once two identities have hashed to the key.
 */
function claimInputKey<GraphKey extends string> (
  ctx: RestorerContext<GraphKey>,
  node: RemoteSideEffectsInstallNode<GraphKey>,
  { inputKey, identity }: { inputKey: string, identity: string }
): boolean {
  if (ctx.collisions.has(inputKey)) return false
  const knownIdentity = ctx.identityByInputKey.get(inputKey)
  if (knownIdentity == null) {
    ctx.identityByInputKey.set(inputKey, identity)
    return true
  }
  if (knownIdentity === identity) return true
  // Two different packages hashing to one input key would make the cache
  // ambiguous. The signed payload is bound to a single package identity so
  // nothing incorrect can be restored, but stop trusting the key.
  ctx.opts.warn?.(`Remote side-effects input key collision for ${node.name}@${node.version}; building locally`)
  ctx.collisions.add(inputKey)
  ctx.lookups.delete(inputKey)
  return false
}

/**
 * Decide from the side effects the store already holds for the package.
 * `undefined` means the registry has to be asked.
 */
async function reuseLocalSideEffects<GraphKey extends string> (
  ctx: RestorerContext<GraphKey>,
  { node, candidate, localCacheKey }: RestoreTarget<GraphKey>
): Promise<{ cacheKey: string | undefined } | undefined> {
  const localSideEffects = node.files.sideEffectsMaps?.get(localCacheKey)
  const storedDiff = node.files.sideEffectsDiffs?.get(localCacheKey)
  if (localSideEffects == null) return undefined
  if (storedDiff?.remoteOrigin == null) {
    return ctx.opts.sideEffectsCacheRead ? { cacheKey: undefined } : undefined
  }
  let verified: boolean
  try {
    verified = await storedArtifactIsVerified(ctx, {
      candidate,
      diff: storedDiff,
      files: localSideEffects,
    })
  } catch (err: unknown) {
    ctx.opts.warn?.(`Persisted remote side-effects artifact for ${node.name}@${node.version} could not be checked: ${errorMessage(err)}`)
    return { cacheKey: undefined }
  }
  if (verified) return { cacheKey: localCacheKey }
  node.files.sideEffectsMaps?.delete(localCacheKey)
  node.files.sideEffectsDiffs?.delete(localCacheKey)
  return undefined
}

async function restoreFromRegistry<GraphKey extends string> (
  ctx: RestorerContext<GraphKey>,
  target: RestoreTarget<GraphKey>,
  registryUrl: string
): Promise<string | undefined> {
  const { node, candidate } = target
  const inputKey = candidate.key
  if (node.filesIndexFile != null) {
    trackFilesIndexFile(ctx, inputKey, node.filesIndexFile)
  }
  const storedQuarantine = node.files.remoteSideEffectsQuarantine?.get(registryUrl)
  if (storedQuarantine != null) {
    adoptStoredQuarantine(ctx, { inputKey, digests: storedQuarantine, filesIndexFile: node.filesIndexFile })
  }

  const resolvedArtifact = await lookupArtifact(ctx, candidate)
  if (resolvedArtifact == null) return undefined
  if (ctx.quarantinedEnvelopeDigests.get(inputKey)?.has(resolvedArtifact.envelopeDigest) === true) return undefined
  const artifact = await ctx.artifactLimit(async () => hydrate(ctx, resolvedArtifact, {
    candidate,
    baseFiles: node.files.filesMap.keys(),
  }))
  if (artifact == null) return undefined
  installRestoredArtifact(ctx, target, artifact)
  return target.localCacheKey
}

function installRestoredArtifact<GraphKey extends string> (
  ctx: RestorerContext<GraphKey>,
  { node, localCacheKey }: RestoreTarget<GraphKey>,
  artifact: RestoredArtifact
): void {
  node.files.sideEffectsMaps ??= new Map()
  node.files.sideEffectsMaps.set(localCacheKey, artifact.files)
  node.files.sideEffectsDiffs ??= new Map()
  node.files.sideEffectsDiffs.set(localCacheKey, artifact.sideEffects)
  if (node.filesIndexFile == null) return
  try {
    ctx.opts.storeController.persistRemoteSideEffects?.({
      filesIndexFile: node.filesIndexFile,
      sideEffectsCacheKey: localCacheKey,
      sideEffects: artifact.sideEffects,
    })
  } catch (err: unknown) {
    ctx.opts.warn?.(`Remote side-effects artifact for ${node.name}@${node.version} could not be persisted: ${errorMessage(err)}`)
  }
}

export interface PublishBuiltSharedSideEffectsOptions<GraphKey extends string> {
  configByUri: Record<string, RegistryConfig>
  depsGraph: DepsGraph<GraphKey>
  graphKey: GraphKey
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

export async function publishBuiltSharedSideEffects<GraphKey extends string> (
  opts: PublishBuiltSharedSideEffectsOptions<GraphKey>
): Promise<void> {
  const publication = publicationInputs(opts)
  if (publication == null) return
  const manifest = await artifactManifest(opts.upload)
  if (manifest == null) return
  const inputKey = calcDepStateInputKey({
    depsGraph: opts.depsGraph,
    depPath: opts.graphKey,
    patchFileHash: opts.patchFileHash,
    supportedArchitectures: opts.supportedArchitectures,
  })
  const payload = createArtifactPayload(opts, { publication, inputKey, manifest: manifest.manifest })
  await publishSharedSideEffects({
    registryUrl: publication.pnprServer,
    authorization: createGetAuthHeaderByURI(opts.configByUri)(publication.pnprServer),
    key: inputKey,
    envelope: createSignedArtifactEnvelope(payload, {
      keyId: publication.keyId,
      privateKey: createPrivateKey({
        key: Buffer.from(publication.privateKey, 'base64'),
        format: 'der',
        type: 'pkcs8',
      }),
    }),
    blobs: manifest.blobs,
  })
}

function publicationInputs<GraphKey extends string> (
  opts: PublishBuiltSharedSideEffectsOptions<GraphKey>
): PublicationInputs | undefined {
  if (
    opts.settings?.publish !== true ||
    opts.pnprServer == null ||
    opts.settings.packages?.includes(opts.name) !== true
  ) return undefined
  const { builderId, keyId, org: organization, privateKey } = opts.settings
  if (!isNonEmpty(organization)) return undefined
  const artifactPlatform = currentArtifactPlatform(opts.nodeVersion)
  const sourceIntegrity = verifiedIntegrity(opts.resolution)
  if (keyId == null || privateKey == null || builderId == null || artifactPlatform == null || sourceIntegrity == null) return undefined
  return {
    pnprServer: opts.pnprServer,
    settings: opts.settings,
    organization,
    builderId,
    keyId,
    privateKey,
    artifactPlatform,
    sourceIntegrity,
  }
}

function createArtifactPayload<GraphKey extends string> (
  opts: PublishBuiltSharedSideEffectsOptions<GraphKey>,
  { publication, inputKey, manifest }: { publication: PublicationInputs, inputKey: string, manifest: ArtifactManifest }
): ArtifactPayload {
  const { settings } = publication
  return {
    kind: 'dependency-side-effects:v1',
    subject: {
      kind: 'dependency-side-effects',
      package: { name: opts.name, version: opts.version },
      sourceIntegrity: publication.sourceIntegrity,
    },
    inputKey,
    owner: { type: 'organization', name: publication.organization },
    builderId: publication.builderId,
    builderProfile: {
      imageDigest: settings.imageDigest,
      architectureBaseline: settings.architectureBaseline ?? process.arch,
      environment: settings.buildEnv ?? {},
    },
    compatibility: {
      kind: 'tagged',
      tags: [artifactCompatibilityTag(publication.artifactPlatform)],
    },
    manifest,
  }
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

function verifiedIntegrity (resolution: LockfileResolution): string | undefined {
  const value = resolution as { type?: string, integrity?: unknown }
  return (value.type == null || value.type === 'binary') && typeof value.integrity === 'string'
    ? value.integrity
    : undefined
}

/**
 * An owner scope needs a name, and `org: ''` is not one. The Rust client
 * refuses it through `non_empty`; this is the same gate.
 */
function isNonEmpty (value: string | undefined): value is string {
  return value != null && value.length > 0
}
