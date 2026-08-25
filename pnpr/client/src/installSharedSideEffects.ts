import { createPrivateKey } from 'node:crypto'
import fs from 'node:fs/promises'
import util from 'node:util'

import { calcDepState, calcDepStateInputKey, type DepsGraph, type DepsStateCache } from '@pnpm/deps.graph-hasher'
import type { LockfileResolution } from '@pnpm/lockfile.types'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import type { PackageFilesResponse, StoreController, UploadPkgToStoreResult } from '@pnpm/store.controller-types'
import type { AllowBuild, DepPath, RegistryConfig, SharedSideEffectsCacheSettings, SupportedArchitectures } from '@pnpm/types'
import pLimit from 'p-limit'

import {
  type ArtifactBlobUpload,
  type ArtifactCandidate,
  type ArtifactManifest,
  type ArtifactPayload,
  createSignedArtifactEnvelope,
  downloadSharedArtifactBlob,
  linuxGlibcCompatibilityTag,
  linuxGlibcSupportedTags,
  pnprSupportsSharedSideEffects,
  publishSharedSideEffects,
  resolveSharedSideEffects,
} from './sharedSideEffects.js'

export interface SharedSideEffectsInstallNode<T extends string> {
  graphKey: T
  depPath: DepPath
  files: PackageFilesResponse
  name: string
  patchFileHash?: string
  resolution: LockfileResolution
  version: string
}

export interface SharedSideEffectsInstallOptions<T extends string> {
  allowBuild?: AllowBuild
  configByUri: Record<string, RegistryConfig>
  depsGraph: DepsGraph<T>
  depsStateCache: DepsStateCache
  ignoreScripts: boolean
  nodeVersion?: string
  nodes: Array<SharedSideEffectsInstallNode<T>>
  pnprServer?: string
  settings?: SharedSideEffectsCacheSettings
  sideEffectsCacheRead: boolean
  storeController: StoreController
  supportedArchitectures?: SupportedArchitectures
  warn?: (message: string) => void
}

export interface SharedSideEffectsInstallPrerequisites {
  ignoreScripts: boolean
  nodeVersion?: string
  pnprServer?: string
  settings?: SharedSideEffectsCacheSettings
  storeController: StoreController
}

export function canApplySharedSideEffectsToInstall (
  opts: SharedSideEffectsInstallPrerequisites
): boolean {
  return opts.pnprServer != null &&
    opts.settings != null &&
    opts.settings.packages.length > 0 &&
    !opts.ignoreScripts &&
    currentLinuxGlibcPlatform(opts.nodeVersion) != null &&
    opts.storeController.addFileToStore != null
}

export async function applySharedSideEffectsToInstall<T extends string> (
  opts: SharedSideEffectsInstallOptions<T>
): Promise<Map<T, string>> {
  if (!canApplySharedSideEffectsToInstall(opts)) return new Map()
  const platform = currentLinuxGlibcPlatform(opts.nodeVersion)
  const { pnprServer, settings } = opts
  if (platform == null || pnprServer == null || settings == null) return new Map()

  const eligiblePackages = new Set(settings.packages)
  const allowedBuilds = new Set<string>()
  const grouped = new Map<string, {
    candidate: ArtifactCandidate
    localCacheKey: string
    nodes: Array<SharedSideEffectsInstallNode<T>>
  }>()
  const collisions = new Set<string>()
  for (const node of opts.nodes) {
    if (!node.files.requiresBuild || !eligiblePackages.has(node.name)) continue
    if (opts.allowBuild?.(node.depPath) !== true) continue
    const sourceIntegrity = verifiedIntegrity(node.resolution)
    if (sourceIntegrity == null) continue
    allowedBuilds.add(node.name)
    const inputKey = calcDepStateInputKey({
      depsGraph: opts.depsGraph,
      depPath: node.graphKey,
      patchFileHash: node.patchFileHash,
      supportedArchitectures: opts.supportedArchitectures,
    })
    const localCacheKey = calcDepState(opts.depsGraph, opts.depsStateCache, node.graphKey, {
      includeDepGraphHash: true,
      patchFileHash: node.patchFileHash,
      supportedArchitectures: opts.supportedArchitectures,
      nodeVersion: opts.nodeVersion,
    })
    if (opts.sideEffectsCacheRead && node.files.sideEffectsMaps?.has(localCacheKey) === true) continue
    if (collisions.has(inputKey)) continue
    const existing = grouped.get(inputKey)
    if (existing != null) {
      if (
        existing.candidate.package.name !== node.name ||
        existing.candidate.package.version !== node.version ||
        existing.candidate.sourceIntegrity !== sourceIntegrity
      ) {
        opts.warn?.(`Shared side-effects input key collision for ${node.name}@${node.version}; building locally`)
        grouped.delete(inputKey)
        collisions.add(inputKey)
        continue
      }
      existing.nodes.push(node)
      continue
    }
    grouped.set(inputKey, {
      candidate: {
        key: inputKey,
        package: { name: node.name, version: node.version },
        sourceIntegrity,
        owner: { type: 'organization', name: settings.organization },
      },
      localCacheKey,
      nodes: [node],
    })
  }
  if (grouped.size === 0) return new Map()

  const authorization = createGetAuthHeaderByURI(opts.configByUri)(pnprServer)
  try {
    if (!await pnprSupportsSharedSideEffects({
      registryUrl: pnprServer,
      authorization,
    })) return new Map()
  } catch (err: unknown) {
    opts.warn?.(`Shared side-effects cache handshake failed: ${errorMessage(err)}`)
    return new Map()
  }

  let resolved
  try {
    resolved = await resolveSharedSideEffects({
      registryUrl: pnprServer,
      authorization,
      candidates: Array.from(grouped.values(), ({ candidate }) => candidate),
      supportedTags: linuxGlibcSupportedTags(platform),
      policy: {
        ignoreScripts: false,
        eligiblePackages,
        allowedBuilds,
      },
      trustedKeys: settings.trustedKeys,
    })
  } catch (err: unknown) {
    opts.warn?.(`Shared side-effects cache lookup failed: ${errorMessage(err)}`)
    return new Map()
  }

  const hits = new Map<T, string>()
  const downloadLimit = pLimit(16)
  const blobs = new Map<string, Promise<Buffer>>()
  const storedBlobs = new Map<string, string>()
  await Promise.all(Array.from(resolved, async ([inputKey, artifact]) => {
    const group = grouped.get(inputKey)
    if (group == null) return
    try {
      const downloaded = await Promise.all(Array.from(
        new Set(artifact.payload.manifest.added.map(file => file.integrity)),
        async (integrity) => {
          let blob = blobs.get(integrity)
          if (blob == null) {
            blob = downloadLimit(async () => downloadSharedArtifactBlob({
              registryUrl: opts.pnprServer!,
              authorization,
              request: {
                owner: artifact.payload.owner,
                integrity,
              },
            }))
            blobs.set(integrity, blob)
          }
          return [integrity, await blob] as const
        }
      ))
      const artifactBlobs = new Map(downloaded)
      const added = new Map<string, string>()
      for (const file of artifact.payload.manifest.added) {
        const storedKey = `${file.integrity}\0${file.mode}`
        let storedPath = storedBlobs.get(storedKey)
        if (storedPath == null) {
          storedPath = opts.storeController.addFileToStore!(artifactBlobs.get(file.integrity)!, file.mode).filePath
          storedBlobs.set(storedKey, storedPath)
        }
        added.set(file.path, storedPath)
      }
      for (const node of group.nodes) {
        node.files.sideEffectsMaps ??= new Map()
        node.files.sideEffectsMaps.set(group.localCacheKey, {
          added,
          deleted: artifact.payload.manifest.deleted,
        })
        hits.set(node.graphKey, group.localCacheKey)
      }
    } catch (err: unknown) {
      opts.warn?.(`Shared side-effects artifact for ${group.candidate.package.name}@${group.candidate.package.version} was rejected: ${errorMessage(err)}`)
    }
  }))
  return hits
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
  settings?: SharedSideEffectsCacheSettings
  supportedArchitectures?: SupportedArchitectures
  upload: UploadPkgToStoreResult
  version: string
}

export async function publishBuiltSharedSideEffects<T extends string> (
  opts: PublishBuiltSharedSideEffectsOptions<T>
): Promise<void> {
  if (
    process.env.PNPM_SHARED_SIDE_EFFECTS_CACHE_PUBLISH !== 'true' ||
    opts.pnprServer == null ||
    opts.settings == null ||
    !opts.settings.packages.includes(opts.name)
  ) return
  const keyId = process.env.PNPM_SHARED_SIDE_EFFECTS_CACHE_KEY_ID
  const privateKey = process.env.PNPM_SHARED_SIDE_EFFECTS_CACHE_PRIVATE_KEY
  const builderId = process.env.PNPM_SHARED_SIDE_EFFECTS_CACHE_BUILDER_ID
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
    owner: { type: 'organization', name: opts.settings.organization },
    builderId,
    builderProfile: {
      imageDigest: process.env.PNPM_SHARED_SIDE_EFFECTS_CACHE_IMAGE_DIGEST,
      architectureBaseline: process.env.PNPM_SHARED_SIDE_EFFECTS_CACHE_ARCHITECTURE_BASELINE ?? process.arch,
      environment: parseBuilderEnvironment(process.env.PNPM_SHARED_SIDE_EFFECTS_CACHE_BUILD_ENV),
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

function parseBuilderEnvironment (value: string | undefined): Record<string, string> {
  if (value == null) return {}
  const parsed = JSON.parse(value) as unknown
  if (parsed == null || typeof parsed !== 'object' || Array.isArray(parsed)) {
    throw new Error('PNPM_SHARED_SIDE_EFFECTS_CACHE_BUILD_ENV must be a JSON object')
  }
  if (!Object.values(parsed).every(item => typeof item === 'string')) {
    throw new Error('PNPM_SHARED_SIDE_EFFECTS_CACHE_BUILD_ENV values must be strings')
  }
  return parsed as Record<string, string>
}

function errorMessage (err: unknown): string {
  return util.types.isNativeError(err) ? err.message : String(err)
}
