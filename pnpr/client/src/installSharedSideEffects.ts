import { execFileSync } from 'node:child_process'
import { createPrivateKey } from 'node:crypto'
import fs from 'node:fs/promises'
import { release as osRelease } from 'node:os'
import util from 'node:util'

import { calcDepState, calcDepStateInputKey, type DepsGraph, type DepsStateCache } from '@pnpm/deps.graph-hasher'
import type { LockfileResolution } from '@pnpm/lockfile.types'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import type { PackageFilesResponse, RemoteSideEffectsOrigin, SideEffectsDiff, StoreController, UploadPkgToStoreResult } from '@pnpm/store.controller-types'
import type { AllowBuild, DepPath, RegistryConfig, RemoteSideEffectsCacheSettings, SupportedArchitectures } from '@pnpm/types'
import pLimit from 'p-limit'

import {
  artifactBlobDigest,
  type ArtifactBlobUpload,
  type ArtifactManifest,
  type ArtifactPayload,
  createSignedArtifactEnvelope,
  type DependencySideEffectsCandidate,
  downloadSharedArtifactBlob,
  linuxGlibcCompatibilityTag,
  type LinuxGlibcPlatform,
  linuxGlibcSupportedTags,
  macOSCompatibilityTag,
  type MacOSPlatform,
  macOSSupportedTags,
  pnprSupportsSharedSideEffects,
  publishSharedSideEffects,
  resolveSharedSideEffects,
  SharedArtifactBlobIntegrityError,
  type SignedArtifactEnvelope,
  type VerifiedArtifact,
  verifyStoredSharedSideEffects,
  windowsCompatibilityTag,
  type WindowsPlatform,
  windowsSupportedTags,
} from './sharedSideEffects.js'

export interface RemoteSideEffectsInstallNode<T extends string> {
  graphKey: T
  depPath: DepPath
  files: PackageFilesResponse
  filesIndexFile?: string
  name: string
  patchFileHash?: string
  resolution: LockfileResolution
  version: string
}

export interface RemoteSideEffectsRestorerOptions<T extends string> {
  allowBuild?: AllowBuild
  configByUri: Record<string, RegistryConfig>
  depsGraph: DepsGraph<T>
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

type ArtifactPlatform =
  | { kind: 'linuxGlibc', platform: LinuxGlibcPlatform }
  | { kind: 'macOS', platform: MacOSPlatform }
  | { kind: 'windows', platform: WindowsPlatform }

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
  sideEffects: SideEffectsDiff
}

interface QueuedLookup {
  candidate: DependencySideEffectsCandidate
  resolve: (artifact: VerifiedArtifact | undefined) => void
}

export function canRestoreRemoteSideEffects (opts: RemoteSideEffectsPrerequisites): boolean {
  return opts.settings != null &&
    isNonEmpty(opts.settings.org) &&
    (opts.settings.packages?.length ?? 0) > 0 &&
    Object.keys(opts.settings.trustedKeys ?? {}).length > 0 &&
    !opts.ignoreScripts &&
    currentArtifactPlatform(opts.nodeVersion) != null
}

export function createRemoteSideEffectsRestorer<T extends string> (
  opts: RemoteSideEffectsRestorerOptions<T>
): RemoteSideEffectsRestorer<T> | undefined {
  if (!canRestoreRemoteSideEffects(opts)) return undefined
  const artifactPlatform = currentArtifactPlatform(opts.nodeVersion)
  const { pnprServer, settings } = opts
  const organization = settings?.org
  if (artifactPlatform == null || settings == null || !isNonEmpty(organization)) return undefined
  const registryUrl = pnprServer
  const ownerName = organization
  const owner = { type: 'organization', name: ownerName } as const
  let supportedTags: string[]
  try {
    supportedTags = artifactSupportedTags(artifactPlatform)
  } catch (err: unknown) {
    opts.warn?.(`Remote side-effects platform is unsupported: ${errorMessage(err)}`)
    return undefined
  }
  const trustedKeys = settings.trustedKeys ?? {}
  const eligiblePackages = new Set(settings.packages)
  const authorization = registryUrl == null ? undefined : createGetAuthHeaderByURI(opts.configByUri)(registryUrl)
  const artifactLimit = pLimit(4)
  const downloadLimit = pLimit(16)
  // A store probe reads and hashes the candidate, so it holds a descriptor for
  // as long as the file takes. A manifest may list `MAX_MANIFEST_FILES` paths
  // and several artifacts hydrate at once, so probing every file the moment it
  // is asked for would exhaust the descriptor table.
  const storeLookupLimit = pLimit(16)
  // Restorer-lifetime, so one blob shared by several artifacts is fetched and
  // stored once however the batches happen to fall.
  const storedBlobs = new Map<string, Promise<{ filePath: string, fileInfo: { checkedAt?: number, digest: string, mode: number, size: number } }>>()
  const identityByInputKey = new Map<string, string>()
  const collisions = new Set<string>()
  const lookups = new Map<string, Promise<VerifiedArtifact | undefined>>()
  const filesIndexFilesByInputKey = new Map<string, Set<string>>()
  const quarantinedEnvelopeDigests = new Map<string, Set<string>>()
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
    const candidate: DependencySideEffectsCandidate = {
      key: inputKey,
      subject: {
        kind: 'dependency-side-effects',
        package: { name: node.name, version: node.version },
        sourceIntegrity,
      },
      owner,
    }
    const localSideEffects = node.files.sideEffectsMaps?.get(localCacheKey)
    const storedDiff = node.files.sideEffectsDiffs?.get(localCacheKey)
    if (localSideEffects != null) {
      if (storedDiff?.remoteOrigin == null) {
        if (opts.sideEffectsCacheRead) return undefined
      } else {
        let verified: boolean
        try {
          verified = await storedArtifactIsVerified({
            candidate,
            diff: storedDiff,
            files: localSideEffects,
          })
        } catch (err: unknown) {
          opts.warn?.(`Persisted remote side-effects artifact for ${node.name}@${node.version} could not be checked: ${errorMessage(err)}`)
          return undefined
        }
        if (verified) return localCacheKey
        node.files.sideEffectsMaps?.delete(localCacheKey)
        node.files.sideEffectsDiffs?.delete(localCacheKey)
      }
    }
    if (registryUrl == null || opts.storeController.addFileToStore == null) return undefined

    if (node.filesIndexFile != null) {
      let filesIndexFiles = filesIndexFilesByInputKey.get(inputKey)
      if (filesIndexFiles == null) {
        filesIndexFiles = new Set()
        filesIndexFilesByInputKey.set(inputKey, filesIndexFiles)
      }
      const isNewFilesIndexFile = !filesIndexFiles.has(node.filesIndexFile)
      filesIndexFiles.add(node.filesIndexFile)
      if (isNewFilesIndexFile) {
        for (const digest of quarantinedEnvelopeDigests.get(inputKey) ?? []) {
          persistQuarantine(node.filesIndexFile, digest)
        }
      }
    }
    const storedQuarantine = node.files.remoteSideEffectsQuarantine?.get(registryUrl)
    if (storedQuarantine != null) {
      let quarantined = quarantinedEnvelopeDigests.get(inputKey)
      if (quarantined == null) {
        quarantined = new Set()
        quarantinedEnvelopeDigests.set(inputKey, quarantined)
      }
      for (const digest of storedQuarantine) {
        if (quarantined.has(digest)) continue
        quarantined.add(digest)
        for (const filesIndexFile of filesIndexFilesByInputKey.get(inputKey) ?? []) {
          if (filesIndexFile !== node.filesIndexFile) persistQuarantine(filesIndexFile, digest)
        }
      }
    }

    let lookup = lookups.get(inputKey)
    if (lookup == null) {
      lookup = enqueue(candidate)
      lookups.set(inputKey, lookup)
    }
    const resolvedArtifact = await lookup
    if (resolvedArtifact == null) return undefined
    if (quarantinedEnvelopeDigests.get(inputKey)?.has(resolvedArtifact.envelopeDigest) === true) return undefined
    const artifact = await artifactLimit(async () => hydrate(resolvedArtifact, candidate))
    if (artifact == null) return undefined
    node.files.sideEffectsMaps ??= new Map()
    node.files.sideEffectsMaps.set(localCacheKey, { added: artifact.added, deleted: artifact.deleted })
    node.files.sideEffectsDiffs ??= new Map()
    node.files.sideEffectsDiffs.set(localCacheKey, artifact.sideEffects)
    if (node.filesIndexFile != null) {
      try {
        opts.storeController.persistRemoteSideEffects?.({
          filesIndexFile: node.filesIndexFile,
          sideEffectsCacheKey: localCacheKey,
          sideEffects: artifact.sideEffects,
        })
      } catch (err: unknown) {
        opts.warn?.(`Remote side-effects artifact for ${node.name}@${node.version} could not be persisted: ${errorMessage(err)}`)
      }
    }
    return localCacheKey
  }

  async function enqueue (candidate: DependencySideEffectsCandidate): Promise<VerifiedArtifact | undefined> {
    let resolve!: (artifact: VerifiedArtifact | undefined) => void
    const promise = new Promise<VerifiedArtifact | undefined>((settle) => {
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
    if (registryUrl == null) {
      for (const { resolve } of batch) resolve(undefined)
      return
    }
    supported ??= (async () => {
      try {
        return await pnprSupportsSharedSideEffects({ registryUrl, authorization })
      } catch (err: unknown) {
        opts.warn?.(`Remote side-effects cache handshake failed: ${errorMessage(err)}`)
        return false
      }
    })()
    if (!await supported) {
      for (const { resolve } of batch) resolve(undefined)
      return
    }
    let resolved
    try {
      resolved = await resolveSharedSideEffects({
        registryUrl,
        authorization,
        candidates: batch.map(({ candidate }) => candidate),
        supportedTags,
        policy: {
          ignoreScripts: false,
          eligiblePackages,
          allowedBuilds: new Set(batch.map(({ candidate }) => candidate.subject.package.name)),
        },
        trustedKeys,
        quarantinedEnvelopeDigests,
        onRejectedArtifact: ({ inputKey, envelopeDigest, reason }) => {
          quarantine(inputKey, envelopeDigest, reason)
        },
      })
    } catch (err: unknown) {
      opts.warn?.(`Remote side-effects cache lookup failed: ${errorMessage(err)}`)
      for (const { resolve } of batch) resolve(undefined)
      return
    }
    await Promise.all(batch.map(async ({ candidate, resolve }) => {
      const artifact = resolved.get(candidate.key)
      if (artifact == null) {
        resolve(undefined)
        return
      }
      resolve(artifact)
    }))
  }

  async function hydrate (
    artifact: VerifiedArtifact,
    candidate: DependencySideEffectsCandidate
  ): Promise<RestoredArtifact | undefined> {
    if (registryUrl == null) return undefined
    try {
      const hydrated = await Promise.all(artifact.payload.manifest.added.map(async (file) => {
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
            if (present != null) {
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
            const bytes = await downloadLimit(async () => downloadSharedArtifactBlob({
              registryUrl,
              authorization,
              request: {
                owner: artifact.payload.owner,
                integrity: file.integrity,
              },
            }))
            if (bytes.byteLength !== file.size) {
              throw new SharedArtifactBlobIntegrityError('Downloaded shared artifact blob does not match its declared size')
            }
            const storedFile = opts.storeController.addFileToStore!(bytes, file.mode)
            return {
              filePath: storedFile.filePath,
              fileInfo: {
                checkedAt: storedFile.checkedAt,
                digest: storedFile.digest,
                mode: file.mode,
                size: file.size,
              },
            }
          })()
          storedBlobs.set(storedKey, stored)
        }
        try {
          const result = await stored
          if (result.fileInfo.size !== file.size) {
            throw new SharedArtifactBlobIntegrityError('Shared artifact blob is declared with inconsistent sizes')
          }
          return [file.path, result] as const
        } catch (err: unknown) {
          if (storedBlobs.get(storedKey) === stored) storedBlobs.delete(storedKey)
          throw err
        }
      }))
      const added = new Map(hydrated.map(([filePath, stored]) => [filePath, stored.filePath]))
      const remoteOrigin: RemoteSideEffectsOrigin = {
        channel: registryUrl,
        owner: artifact.payload.owner,
        signerKeyId: artifact.envelope.keyId,
        builderProfile: artifact.payload.builderProfile,
        envelope: artifact.envelope,
        verification: 'verified',
      }
      const sideEffects: SideEffectsDiff = {
        added: new Map(hydrated.map(([filePath, stored]) => [filePath, stored.fileInfo])),
        deleted: artifact.payload.manifest.deleted,
        remoteOrigin,
      }
      return {
        added,
        deleted: artifact.payload.manifest.deleted,
        sideEffects,
      }
    } catch (err: unknown) {
      if (isBlobIntegrityError(err)) {
        quarantine(candidate.key, artifact.envelopeDigest, errorMessage(err))
        return undefined
      }
      opts.warn?.(`Remote side-effects artifact for ${candidate.subject.package.name}@${candidate.subject.package.version} was rejected: ${errorMessage(err)}`)
      return undefined
    }
  }

  async function storedArtifactIsVerified (params: {
    candidate: DependencySideEffectsCandidate
    diff: SideEffectsDiff
    files: { added?: Map<string, string>, deleted?: string[] }
  }): Promise<boolean> {
    const { candidate, diff, files } = params
    const origin = diff.remoteOrigin
    if (
      origin == null ||
      origin.verification !== 'verified' ||
      origin.signerKeyId !== origin.envelope.keyId ||
      (registryUrl != null && origin.channel !== registryUrl)
    ) return false
    const publicKey = trustedKeys[origin.signerKeyId]
    if (publicKey == null) return false
    let artifact: VerifiedArtifact
    try {
      artifact = verifyStoredSharedSideEffects({
        candidate,
        envelope: origin.envelope as SignedArtifactEnvelope,
        publicKey,
        supportedTags,
      })
    } catch {
      return false
    }
    if (!ownersMatch(origin.owner, artifact.payload.owner) ||
      !builderProfilesMatch(origin.builderProfile, artifact.payload.builderProfile) ||
      !manifestMatchesDiff(artifact.payload.manifest, diff)) return false
    const validFiles = await Promise.all(Array.from(diff.added ?? [], async ([filePath, info]) => {
      return storeLookupLimit(async () => {
        const located = await opts.storeController.locateFileInStore?.(info.digest, info.mode)
        return located != null &&
          files.added?.get(filePath) === located &&
          (await fs.stat(located)).size === info.size
      })
    }))
    return validFiles.every(Boolean)
  }

  function quarantine (inputKey: string, envelopeDigest: string, reason: string): void {
    if (registryUrl == null) return
    let quarantined = quarantinedEnvelopeDigests.get(inputKey)
    if (quarantined == null) {
      quarantined = new Set()
      quarantinedEnvelopeDigests.set(inputKey, quarantined)
    }
    if (quarantined.has(envelopeDigest)) return
    quarantined.add(envelopeDigest)
    for (const filesIndexFile of filesIndexFilesByInputKey.get(inputKey) ?? []) {
      persistQuarantine(filesIndexFile, envelopeDigest)
    }
    opts.warn?.(`Remote side-effects artifact was quarantined: ${reason}`)
  }

  function persistQuarantine (filesIndexFile: string, envelopeDigest: string): void {
    if (registryUrl == null) return
    try {
      opts.storeController.quarantineRemoteSideEffects?.({
        channel: registryUrl,
        envelopeDigest,
        filesIndexFile,
      })
    } catch (err: unknown) {
      opts.warn?.(`Remote side-effects quarantine could not be persisted: ${errorMessage(err)}`)
    }
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
  const { builderId, keyId, org: organization, privateKey } = opts.settings
  if (!isNonEmpty(organization)) return
  const artifactPlatform = currentArtifactPlatform(opts.nodeVersion)
  const sourceIntegrity = verifiedIntegrity(opts.resolution)
  if (keyId == null || privateKey == null || builderId == null || artifactPlatform == null || sourceIntegrity == null) return
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
    subject: {
      kind: 'dependency-side-effects',
      package: { name: opts.name, version: opts.version },
      sourceIntegrity,
    },
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
      tags: [artifactCompatibilityTag(artifactPlatform)],
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

function currentArtifactPlatform (nodeVersion?: string): ArtifactPlatform | undefined {
  if (!['x64', 'arm64'].includes(process.arch)) return undefined
  const version = nodeVersion ?? process.version
  const nodeMajor = Number((version.startsWith('v') ? version.slice(1) : version).split('.')[0])
  if (!Number.isSafeInteger(nodeMajor) || nodeMajor <= 0) return undefined
  if (process.platform === 'linux') {
    const report = process.report?.getReport() as { header?: { glibcVersionRuntime?: string } }
    const [glibcMajor, glibcMinor] = report.header?.glibcVersionRuntime?.split('.').map(Number) ?? []
    if (![glibcMajor, glibcMinor].every(Number.isSafeInteger)) return undefined
    return {
      kind: 'linuxGlibc',
      platform: { architecture: process.arch, nodeMajor, glibcMajor, glibcMinor },
    }
  }
  if (process.platform === 'darwin') {
    const version = macOSProductVersion()
    if (version == null) return undefined
    return {
      kind: 'macOS',
      platform: {
        architecture: process.arch,
        nodeMajor,
        macOSMajor: version.major,
        macOSMinor: version.minor,
      },
    }
  }
  if (process.platform === 'win32') {
    const version = windowsKernelVersion(osRelease())
    if (version == null) return undefined
    return {
      kind: 'windows',
      platform: {
        architecture: process.arch,
        nodeMajor,
        windowsMajor: version.major,
        windowsMinor: version.minor,
        windowsBuild: version.build,
      },
    }
  }
  return undefined
}

function artifactCompatibilityTag (artifactPlatform: ArtifactPlatform): string {
  switch (artifactPlatform.kind) {
    case 'linuxGlibc': return linuxGlibcCompatibilityTag(artifactPlatform.platform)
    case 'macOS': return macOSCompatibilityTag(artifactPlatform.platform)
    case 'windows': return windowsCompatibilityTag(artifactPlatform.platform)
  }
}

function artifactSupportedTags (artifactPlatform: ArtifactPlatform): string[] {
  switch (artifactPlatform.kind) {
    case 'linuxGlibc': return linuxGlibcSupportedTags(artifactPlatform.platform)
    case 'macOS': return macOSSupportedTags(artifactPlatform.platform)
    case 'windows': return windowsSupportedTags(artifactPlatform.platform)
  }
}

let cachedMacOSProductVersion: { major: number, minor: number } | null | undefined

function macOSProductVersion (): { major: number, minor: number } | undefined {
  if (cachedMacOSProductVersion !== undefined) return cachedMacOSProductVersion ?? undefined
  try {
    const [major, minor] = execFileSync('/usr/bin/sw_vers', ['-productVersion'], {
      encoding: 'utf8',
      timeout: 5_000,
    }).trim().split('.').map(Number)
    cachedMacOSProductVersion =
      Number.isSafeInteger(major) && major > 0 && major < 1_000_000 &&
      Number.isSafeInteger(minor) && minor >= 0 && minor < 1_000_000
        ? { major, minor }
        : null
  } catch {
    cachedMacOSProductVersion = null
  }
  return cachedMacOSProductVersion ?? undefined
}

function windowsKernelVersion (release: string): { major: number, minor: number, build: number } | undefined {
  const components = release.split('.')
  if (components.length !== 3) return undefined
  const [major, minor, build] = components.map(Number)
  if (
    !Number.isSafeInteger(major) || major <= 0 || major >= 1_000 ||
    !Number.isSafeInteger(minor) || minor < 0 || minor >= 1_000 ||
    !Number.isSafeInteger(build) || build <= 0 || build >= 1_000_000
  ) return undefined
  return { major, minor, build }
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

function isBlobIntegrityError (err: unknown): boolean {
  return util.types.isNativeError(err) &&
    'code' in err &&
    err.code === 'ERR_PNPM_SHARED_ARTIFACT_BLOB_INTEGRITY'
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

/**
 * An owner scope needs a name, and `org: ''` is not one. The Rust client
 * refuses it through `non_empty`; this is the same gate.
 */
function isNonEmpty (value: string | undefined): value is string {
  return value != null && value.length > 0
}
