import type { DepsGraph, DepsStateCache } from '@pnpm/deps.graph-hasher'
import type { LockfileResolution } from '@pnpm/lockfile.types'
import type { SideEffectsFilesMap } from '@pnpm/store.cafs'
import type { PackageFilesResponse, SideEffectsDiff, StoreController } from '@pnpm/store.controller-types'
import type { AllowBuild, DepPath, RegistryConfig, RemoteSideEffectsCacheSettings, SupportedArchitectures } from '@pnpm/types'
import type { LimitFunction } from 'p-limit'

import { errorMessage } from './errorMessage.js'
import type { DependencySideEffectsCandidate, VerifiedArtifact } from './sharedSideEffects.js'

export interface RemoteSideEffectsInstallNode<GraphKey extends string> {
  graphKey: GraphKey
  depPath: DepPath
  files: PackageFilesResponse
  filesIndexFile?: string
  name: string
  patchFileHash?: string
  resolution: LockfileResolution
  version: string
}

export interface RemoteSideEffectsRestorerOptions<GraphKey extends string> {
  allowBuild?: AllowBuild
  configByUri: Record<string, RegistryConfig>
  depsGraph: DepsGraph<GraphKey>
  depsStateCache: DepsStateCache
  ignoreScripts: boolean
  nodeVersion?: string
  pnprServer?: string
  settings?: RemoteSideEffectsCacheSettings
  sideEffectsCacheRead: boolean
  storeController: StoreController
  supportedArchitectures?: SupportedArchitectures
  warn?: (message: string) => void
}

export interface RestoredArtifact {
  files: SideEffectsFilesMap
  sideEffects: SideEffectsDiff
}

export interface StagedBlob {
  filePath: string
  fileInfo: { checkedAt?: number, digest: string, mode: number, size: number }
}

export interface QueuedLookup {
  candidate: DependencySideEffectsCandidate
  resolve: (artifact: VerifiedArtifact | undefined) => void
}

export interface LookupQueue {
  queued: QueuedLookup[]
  flushTimer?: NodeJS.Timeout
  supported?: Promise<boolean>
}

export interface ArtifactRejection {
  inputKey: string
  envelopeDigest: string
  reason: string
}

/**
 * The state one restorer shares across every package it restores.
 */
export interface RestorerContext<GraphKey extends string> {
  opts: RemoteSideEffectsRestorerOptions<GraphKey>
  registryUrl: string | undefined
  authorization: string | undefined
  owner: { readonly type: 'organization', readonly name: string }
  supportedTags: string[]
  trustedKeys: Record<string, string>
  eligiblePackages: Set<string>
  artifactLimit: LimitFunction
  downloadLimit: LimitFunction
  /**
   * A store probe reads and hashes the candidate, so it holds a descriptor for
   * as long as the file takes. A manifest may list `MAX_MANIFEST_FILES` paths
   * and several artifacts hydrate at once, so probing every file the moment it
   * is asked for would exhaust the descriptor table.
   */
  storeLookupLimit: LimitFunction
  /**
   * Restorer-lifetime, so one blob shared by several artifacts is fetched and
   * stored once however the batches happen to fall.
   */
  storedBlobs: Map<string, Promise<StagedBlob>>
  identityByInputKey: Map<string, string>
  collisions: Set<string>
  lookups: Map<string, Promise<VerifiedArtifact | undefined>>
  filesIndexFilesByInputKey: Map<string, Set<string>>
  quarantinedEnvelopeDigests: Map<string, Set<string>>
  lookupQueue: LookupQueue
}

type QuarantineContext = Pick<RestorerContext<string>, 'opts' | 'registryUrl' | 'filesIndexFilesByInputKey' | 'quarantinedEnvelopeDigests'>

export function quarantine (ctx: QuarantineContext, rejection: ArtifactRejection): void {
  if (ctx.registryUrl == null) return
  const quarantined = quarantinedDigestsOf(ctx, rejection.inputKey)
  if (quarantined.has(rejection.envelopeDigest)) return
  quarantined.add(rejection.envelopeDigest)
  for (const filesIndexFile of ctx.filesIndexFilesByInputKey.get(rejection.inputKey) ?? []) {
    persistQuarantine(ctx, filesIndexFile, rejection.envelopeDigest)
  }
  ctx.opts.warn?.(`Remote side-effects artifact was quarantined: ${rejection.reason}`)
}

/**
 * Remember `filesIndexFile` as one more store index of `inputKey`'s package,
 * and record there every envelope already quarantined for that key.
 */
export function trackFilesIndexFile (ctx: QuarantineContext, inputKey: string, filesIndexFile: string): void {
  let filesIndexFiles = ctx.filesIndexFilesByInputKey.get(inputKey)
  if (filesIndexFiles == null) {
    filesIndexFiles = new Set()
    ctx.filesIndexFilesByInputKey.set(inputKey, filesIndexFiles)
  }
  const isNewFilesIndexFile = !filesIndexFiles.has(filesIndexFile)
  filesIndexFiles.add(filesIndexFile)
  if (!isNewFilesIndexFile) return
  for (const digest of ctx.quarantinedEnvelopeDigests.get(inputKey) ?? []) {
    persistQuarantine(ctx, filesIndexFile, digest)
  }
}

/**
 * Adopt the quarantine a package's store index already carries, and copy each
 * newly learned digest to the key's other store indexes.
 */
export function adoptStoredQuarantine (
  ctx: QuarantineContext,
  stored: { inputKey: string, digests: Iterable<string>, filesIndexFile?: string }
): void {
  const quarantined = quarantinedDigestsOf(ctx, stored.inputKey)
  for (const digest of stored.digests) {
    if (quarantined.has(digest)) continue
    quarantined.add(digest)
    for (const filesIndexFile of ctx.filesIndexFilesByInputKey.get(stored.inputKey) ?? []) {
      if (filesIndexFile !== stored.filesIndexFile) persistQuarantine(ctx, filesIndexFile, digest)
    }
  }
}

function quarantinedDigestsOf (ctx: QuarantineContext, inputKey: string): Set<string> {
  let quarantined = ctx.quarantinedEnvelopeDigests.get(inputKey)
  if (quarantined == null) {
    quarantined = new Set()
    ctx.quarantinedEnvelopeDigests.set(inputKey, quarantined)
  }
  return quarantined
}

function persistQuarantine (ctx: QuarantineContext, filesIndexFile: string, envelopeDigest: string): void {
  if (ctx.registryUrl == null) return
  try {
    ctx.opts.storeController.quarantineRemoteSideEffects?.({
      channel: ctx.registryUrl,
      envelopeDigest,
      filesIndexFile,
    })
  } catch (err: unknown) {
    ctx.opts.warn?.(`Remote side-effects quarantine could not be persisted: ${errorMessage(err)}`)
  }
}
