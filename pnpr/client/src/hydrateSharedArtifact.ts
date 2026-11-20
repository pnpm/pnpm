import fs from 'node:fs/promises'
import util from 'node:util'

import { createSideEffectsFilesMapBuilder } from '@pnpm/store.cafs'
import type { RemoteSideEffectsOrigin, SideEffectsDiff } from '@pnpm/store.controller-types'

import { errorMessage } from './errorMessage.js'
import {
  quarantine,
  type RestoredArtifact,
  type RestorerContext,
  type StagedBlob,
} from './remoteSideEffectsRestorerContext.js'
import {
  artifactBlobDigest,
  type ArtifactFile,
  type DependencySideEffectsCandidate,
  downloadSharedArtifactBlob,
  SharedArtifactBlobIntegrityError,
  type VerifiedArtifact,
} from './sharedSideEffects.js'

type HydrationContext = RestorerContext<string>

export interface HydrationSource {
  candidate: DependencySideEffectsCandidate
  baseFiles: Iterable<string>
}

interface ArtifactFileSource {
  registryUrl: string
  artifact: VerifiedArtifact
  file: ArtifactFile
}

/**
 * Stage the artifact's blobs in the store and resolve it over the package's
 * `baseFiles` the way a persisted diff is. An artifact that cannot be
 * restored is the artifact's fault and is quarantined.
 */
export async function hydrate (
  ctx: HydrationContext,
  artifact: VerifiedArtifact,
  source: HydrationSource
): Promise<RestoredArtifact | undefined> {
  const registryUrl = ctx.registryUrl
  if (registryUrl == null) return undefined
  const { candidate } = source
  try {
    const staged = await Promise.all(artifact.payload.manifest.added.map(async (file) =>
      stageArtifactFile(ctx, { registryUrl, artifact, file })
    ))
    return assembleRestoredArtifact(ctx, { registryUrl, artifact, staged, source })
  } catch (err: unknown) {
    if (isBlobIntegrityError(err)) {
      quarantine(ctx, { inputKey: candidate.key, envelopeDigest: artifact.envelopeDigest, reason: errorMessage(err) })
      return undefined
    }
    ctx.opts.warn?.(`Remote side-effects artifact for ${candidate.subject.package.name}@${candidate.subject.package.version} was rejected: ${errorMessage(err)}`)
    return undefined
  }
}

async function stageArtifactFile (
  ctx: HydrationContext,
  fileSource: ArtifactFileSource
): Promise<readonly [string, StagedBlob]> {
  const { file } = fileSource
  const storedKey = `${file.integrity}\0${file.mode}`
  let stored = ctx.storedBlobs.get(storedKey)
  if (stored == null) {
    stored = storeArtifactBlob(ctx, fileSource)
    ctx.storedBlobs.set(storedKey, stored)
  }
  try {
    const result = await stored
    if (result.fileInfo.size !== file.size) {
      throw new SharedArtifactBlobIntegrityError('Shared artifact blob is declared with inconsistent sizes')
    }
    return [file.path, result] as const
  } catch (err: unknown) {
    if (ctx.storedBlobs.get(storedKey) === stored) ctx.storedBlobs.delete(storedKey)
    throw err
  }
}

async function storeArtifactBlob (ctx: HydrationContext, fileSource: ArtifactFileSource): Promise<StagedBlob> {
  const { file } = fileSource
  // A built package's files are mostly its own, and artifacts share
  // files with each other. The store addresses content by the digest
  // this manifest entry already carries, so anything it holds is the
  // same bytes and does not need transferring again.
  const present = await ctx.storeLookupLimit(async () => ctx.opts.storeController.locateFileInStore?.(
    artifactBlobDigest(file.integrity),
    file.mode
  ))
  if (present != null) {
    return reuseStoredBlob(present, file)
  }
  return downloadArtifactBlob(ctx, fileSource)
}

async function reuseStoredBlob (present: string, file: ArtifactFile): Promise<StagedBlob> {
  const stat = await fs.stat(present)
  if (stat.size !== file.size) {
    throw new SharedArtifactBlobIntegrityError('Stored shared artifact blob does not match its declared size')
  }
  return {
    filePath: present,
    fileInfo: {
      digest: artifactBlobDigest(file.integrity),
      mode: file.mode,
      size: file.size,
    },
  }
}

async function downloadArtifactBlob (
  ctx: HydrationContext,
  { registryUrl, artifact, file }: ArtifactFileSource
): Promise<StagedBlob> {
  const bytes = await ctx.downloadLimit(async () => downloadSharedArtifactBlob({
    registryUrl,
    authorization: ctx.authorization,
    request: {
      owner: artifact.payload.owner,
      integrity: file.integrity,
    },
  }))
  if (bytes.byteLength !== file.size) {
    throw new SharedArtifactBlobIntegrityError('Downloaded shared artifact blob does not match its declared size')
  }
  const storedFile = ctx.opts.storeController.addFileToStore!(bytes, file.mode)
  return {
    filePath: storedFile.filePath,
    fileInfo: {
      checkedAt: storedFile.checkedAt,
      digest: storedFile.digest,
      mode: file.mode,
      size: file.size,
    },
  }
}

function assembleRestoredArtifact (
  ctx: HydrationContext,
  { registryUrl, artifact, staged, source }: {
    registryUrl: string
    artifact: VerifiedArtifact
    staged: Array<readonly [string, StagedBlob]>
    source: HydrationSource
  }
): RestoredArtifact | undefined {
  const added: NonNullable<SideEffectsDiff['added']> = new Map()
  const builder = createSideEffectsFilesMapBuilder()
  for (const [filePath, stored] of staged) {
    added.set(filePath, stored.fileInfo)
    builder.add(filePath, stored.fileInfo.mode, stored.filePath)
  }
  const deleted = artifact.payload.manifest.deleted
  const files = builder.finish(deleted, source.baseFiles)
  if (files == null) {
    quarantine(ctx, {
      inputKey: source.candidate.key,
      envelopeDigest: artifact.envelopeDigest,
      reason: 'the artifact records an entry that cannot be restored',
    })
    return undefined
  }
  const remoteOrigin: RemoteSideEffectsOrigin = {
    channel: registryUrl,
    owner: artifact.payload.owner,
    signerKeyId: artifact.envelope.keyId,
    builderProfile: artifact.payload.builderProfile,
    envelope: artifact.envelope,
    verification: 'verified',
  }
  return {
    files,
    sideEffects: { added, deleted, remoteOrigin },
  }
}

function isBlobIntegrityError (err: unknown): boolean {
  return util.types.isNativeError(err) &&
    'code' in err &&
    err.code === 'ERR_PNPM_SHARED_ARTIFACT_BLOB_INTEGRITY'
}
