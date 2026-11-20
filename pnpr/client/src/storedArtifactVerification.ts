import fs from 'node:fs/promises'

import { isSymlinkMode, type SideEffectsFilesMap } from '@pnpm/store.cafs'
import type { RemoteSideEffectsOrigin, SideEffectsDiff } from '@pnpm/store.controller-types'

import type { RestorerContext } from './remoteSideEffectsRestorerContext.js'
import {
  artifactBlobDigest,
  type ArtifactManifest,
  type ArtifactPayload,
  type DependencySideEffectsCandidate,
  type SignedArtifactEnvelope,
  type VerifiedArtifact,
  verifyStoredSharedSideEffects,
} from './sharedSideEffects.js'

type VerificationContext = Pick<RestorerContext<string>, 'opts' | 'registryUrl' | 'storeLookupLimit' | 'supportedTags' | 'trustedKeys'>

export interface StoredArtifact {
  candidate: DependencySideEffectsCandidate
  diff: SideEffectsDiff
  files: SideEffectsFilesMap
}

/**
 * Whether a persisted remote artifact still carries a trusted signature for
 * `candidate` and every file it added is still intact in the store.
 */
export async function storedArtifactIsVerified (ctx: VerificationContext, stored: StoredArtifact): Promise<boolean> {
  const { candidate, diff } = stored
  const origin = diff.remoteOrigin
  if (origin == null || !originIsTrusted(origin, ctx.registryUrl)) return false
  const artifact = verifyOriginEnvelope(ctx, candidate, origin)
  if (artifact == null) return false
  if (!ownersMatch(origin.owner, artifact.payload.owner) ||
    !builderProfilesMatch(origin.builderProfile, artifact.payload.builderProfile) ||
    !manifestMatchesDiff(artifact.payload.manifest, diff)) return false
  return storedFilesAreIntact(ctx, stored)
}

function originIsTrusted (origin: RemoteSideEffectsOrigin, registryUrl: string | undefined): boolean {
  return origin.verification === 'verified' &&
    origin.signerKeyId === origin.envelope.keyId &&
    (registryUrl == null || origin.channel === registryUrl)
}

function verifyOriginEnvelope (
  ctx: VerificationContext,
  candidate: DependencySideEffectsCandidate,
  origin: RemoteSideEffectsOrigin
): VerifiedArtifact | undefined {
  const publicKey = ctx.trustedKeys[origin.signerKeyId]
  if (publicKey == null) return undefined
  try {
    return verifyStoredSharedSideEffects({
      candidate,
      envelope: origin.envelope as SignedArtifactEnvelope,
      publicKey,
      supportedTags: ctx.supportedTags,
    })
  } catch {
    return undefined
  }
}

async function storedFilesAreIntact (ctx: VerificationContext, { diff, files }: StoredArtifact): Promise<boolean> {
  const validFiles = await Promise.all(Array.from(diff.added ?? [], async ([filePath, info]) => {
    return ctx.storeLookupLimit(async () => {
      const located = await ctx.opts.storeController.locateFileInStore?.(info.digest, info.mode)
      if (located == null) return false
      const restored = isSymlinkMode(info.mode)
        ? files.symlinks?.has(filePath) === true
        : files.added?.get(filePath) === located
      return restored && (await fs.stat(located)).size === info.size
    })
  }))
  return validFiles.every(Boolean)
}

function ownersMatch (
  left: RemoteSideEffectsOrigin['owner'],
  right: ArtifactPayload['owner']
): boolean {
  if (left.type !== right.type) return false
  return left.type === 'organization'
    ? left.name === (right as { type: 'organization', name: string }).name
    : left.package === (right as { type: 'publisher', package: string }).package
}

function builderProfilesMatch (
  left: RemoteSideEffectsOrigin['builderProfile'],
  right: ArtifactPayload['builderProfile']
): boolean {
  if (
    left.imageDigest !== right.imageDigest ||
    left.architectureBaseline !== right.architectureBaseline
  ) return false
  const leftEnvironment = Object.entries(left.environment)
  const rightEnvironment = Object.entries(right.environment)
  return leftEnvironment.length === rightEnvironment.length &&
    leftEnvironment.every(([name, value]) => right.environment[name] === value)
}

function manifestMatchesDiff (manifest: ArtifactManifest, diff: SideEffectsDiff): boolean {
  const added = diff.added ?? new Map()
  if (added.size !== manifest.added.length) return false
  for (const file of manifest.added) {
    const stored = added.get(file.path)
    if (
      stored == null ||
      stored.digest !== artifactBlobDigest(file.integrity) ||
      stored.mode !== file.mode ||
      stored.size !== file.size
    ) return false
  }
  const deleted = diff.deleted ?? []
  return deleted.length === manifest.deleted.length &&
    new Set(deleted).size === deleted.length &&
    deleted.every(path => manifest.deleted.includes(path))
}
