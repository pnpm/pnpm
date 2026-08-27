import { createHash, generateKeyPairSync } from 'node:crypto'
import fs from 'node:fs/promises'
import { createServer } from 'node:http'
import os from 'node:os'
import path from 'node:path'
import { setTimeout as delay } from 'node:timers/promises'

import { describe, expect, test } from '@jest/globals'
import type { DepsGraph } from '@pnpm/deps.graph-hasher'
import type { LockfileObject, LockfileResolution } from '@pnpm/lockfile.types'
import {
  type ArtifactPayload,
  createRemoteSideEffectsRestorer,
  createSignedArtifactEnvelope,
  linuxGlibcCompatibilityTag,
  type LinuxGlibcPlatform,
  publishBuiltSharedSideEffects,
  type SignedArtifactEnvelope,
  verifySignedArtifactEnvelope,
} from '@pnpm/pnpr.client'
import type { PackageFilesResponse, SideEffectsDiff, StoreController } from '@pnpm/store.controller-types'
import type { DepPath } from '@pnpm/types'

const packageName = 'native-addon'
const packageVersion = '1.0.0'
const graphKey = `${packageName}@${packageVersion}`
const depPath = graphKey as DepPath
const sourceIntegrity = `sha512-${createHash('sha512').update('source').digest('base64')}`
const builtFile = Buffer.from('compiled native addon')
const builtFileIntegrity = `sha512-${createHash('sha512').update(builtFile).digest('base64')}`
describe('install remote side-effects', () => {
  test('hydrates the store and selects a verified remote build', async () => {
    const platform = currentLinuxGlibcPlatform()
    if (platform == null) return

    const { privateKey, publicKey } = generateKeyPairSync('ec', { namedCurve: 'prime256v1' })
    const trustedKeys = {
      'acme-2026': publicKey.export({ format: 'der', type: 'spki' }).toString('base64'),
    }
    const requestedPaths: string[] = []
    const serverState = { corruptBlob: false }
    const envelopesByKey = new Map<string, SignedArtifactEnvelope>()
    let heldResolve: { wait: Promise<void>, notifyStarted: () => void } | undefined
    const server = createServer((request, response) => {
      const chunks: Buffer[] = []
      request.on('data', chunk => chunks.push(Buffer.from(chunk)))
      request.on('end', () => {
        requestedPaths.push(request.url ?? '')
        if (request.url === '/-/pnpr') {
          response.writeHead(200, { 'content-type': 'application/json' })
            .end(JSON.stringify({ pnpr: { versions: [0], artifacts: [0] } }))
          return
        }
        if (request.url === '/-/pnpr/v0/artifacts/resolve') {
          const body = JSON.parse(Buffer.concat(chunks).toString('utf8')) as {
            candidates: Array<Pick<ArtifactPayload, 'inputKey' | 'package' | 'sourceIntegrity' | 'owner'> & { key: string }>
          }
          const envelopes = body.candidates.map((candidate) => {
            const payload: ArtifactPayload = {
              kind: 'dependency-side-effects:v1',
              package: candidate.package,
              sourceIntegrity: candidate.sourceIntegrity,
              inputKey: candidate.key,
              owner: candidate.owner,
              builderId: 'ci/main/42',
              builderProfile: {
                architectureBaseline: process.arch,
                environment: {},
              },
              compatibility: {
                kind: 'tagged',
                tags: [linuxGlibcCompatibilityTag(platform)],
              },
              manifest: {
                added: [
                  {
                    path: 'build/addon.node',
                    integrity: builtFileIntegrity,
                    mode: 0o755,
                    size: builtFile.byteLength,
                  },
                  {
                    path: 'build/addon-copy.node',
                    integrity: builtFileIntegrity,
                    mode: 0o755,
                    size: builtFile.byteLength,
                  },
                ],
                deleted: ['src/intermediate.o'],
              },
            }
            let envelope = envelopesByKey.get(candidate.key)
            if (envelope == null) {
              envelope = createSignedArtifactEnvelope(payload, {
                keyId: 'acme-2026',
                privateKey,
              })
              envelopesByKey.set(candidate.key, envelope)
            }
            return {
              key: candidate.key,
              variants: [{ envelope }],
            }
          })
          const sendResponse = (): void => {
            response.writeHead(200, { 'content-type': 'application/json' })
              .end(JSON.stringify({ artifacts: envelopes }))
          }
          const held = heldResolve
          if (held == null) {
            sendResponse()
          } else {
            heldResolve = undefined
            held.notifyStarted()
            void held.wait.then(sendResponse)
          }
          return
        }
        if (request.url === '/-/pnpr/v0/artifacts/blob') {
          response.writeHead(200, { 'content-type': 'application/octet-stream' })
            .end(serverState.corruptBlob ? Buffer.from('corrupt') : builtFile)
          return
        }
        response.writeHead(404).end()
      })
    })
    const pnprServer = await listen(server)
    const files: PackageFilesResponse = {
      filesMap: new Map(),
      requiresBuild: true,
      resolvedFrom: 'remote',
    }
    const storedFiles: Array<{ bytes: Buffer, mode: number }> = []
    const persisted: Array<{ filesIndexFile: string, sideEffectsCacheKey: string, sideEffects: SideEffectsDiff }> = []
    const quarantined: Array<{ channel: string, envelopeDigest: string, filesIndexFile: string }> = []
    const alreadyInStore = new Map<string, string>()
    const alreadyStoredFile = path.join(
      await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-shared-side-effects-store-')),
      'addon.node'
    )
    await fs.writeFile(alreadyStoredFile, builtFile)
    const storeController = {
      addFileToStore: (bytes: Buffer, mode: number) => {
        storedFiles.push({ bytes, mode })
        return {
          checkedAt: Date.now(),
          digest: createHash('sha512').update(bytes).digest('hex'),
          filePath: '/store/cafs/build-addon.node',
        }
      },
      locateFileInStore: async (hexDigest: string, mode: number) => alreadyInStore.get(`${hexDigest}\0${mode}`),
      persistRemoteSideEffects: (entry: typeof persisted[number]) => {
        persisted.push(entry)
        return true
      },
      quarantineRemoteSideEffects: (entry: typeof quarantined[number]) => {
        quarantined.push(entry)
        return true
      },
    } as unknown as StoreController
    const depsGraph: DepsGraph<typeof graphKey> = {
      [graphKey]: {
        children: {},
        fullPkgId: graphKey,
      },
    }
    const artifactPinsLockfile: LockfileObject = {
      lockfileVersion: '9.0',
      importers: {},
      packages: {
        [depPath]: {
          artifactPins: {
            'dependency-side-effects:v1:deps=other': {
              'organization:acme': { 'other-platform': '1'.repeat(64) },
            },
          },
          resolution: { integrity: sourceIntegrity },
        },
      },
    }
    let artifactPinsChanged = 0

    try {
      const restorer = createRemoteSideEffectsRestorer({
        allowBuild: candidate => candidate === depPath,
        artifactPinsLockfile,
        configByUri: {},
        depsGraph,
        depsStateCache: {},
        ignoreScripts: false,
        pnprServer,
        recordArtifactPins: true,
        settings: {
          organization: 'acme',
          packages: [packageName],
          trustedKeys,
        },
        sideEffectsCacheRead: false,
        storeController,
        onArtifactPinsChanged: () => {
          artifactPinsChanged++
        },
      })
      const cacheKey = await restorer?.restore({
        graphKey,
        depPath,
        files,
        filesIndexFile: 'package-index-row',
        name: packageName,
        resolution: { integrity: sourceIntegrity } as LockfileResolution,
        version: packageVersion,
      })

      expect(cacheKey).toBeDefined()
      expect(files.sideEffectsMaps?.get(cacheKey!)).toEqual({
        added: new Map([
          ['build/addon.node', '/store/cafs/build-addon.node'],
          ['build/addon-copy.node', '/store/cafs/build-addon.node'],
        ]),
        deleted: ['src/intermediate.o'],
      })
      expect(storedFiles).toEqual([{ bytes: builtFile, mode: 0o755 }])
      const recordedPins = artifactPinsLockfile.packages?.[depPath].artifactPins
      expect(Object.keys(recordedPins ?? {})).toHaveLength(2)
      expect(recordedPins?.['dependency-side-effects:v1:deps=other'])
        .toEqual({ 'organization:acme': { 'other-platform': '1'.repeat(64) } })
      const [inputKey, owners] = Object.entries(recordedPins ?? {})
        .find(([key]) => key !== 'dependency-side-effects:v1:deps=other')!
      const ownerPins = owners['organization:acme']
      expect(Object.keys(ownerPins ?? {})).toHaveLength(1)
      expect(Object.keys(ownerPins ?? {})[0]).toMatch(/^[a-f0-9]{64}$/)
      expect(Object.values(ownerPins ?? {})[0]).toMatch(/^[a-f0-9]{64}$/)
      expect(artifactPinsChanged).toBe(1)
      expect(persisted).toHaveLength(1)
      expect(persisted[0]).toMatchObject({
        filesIndexFile: 'package-index-row',
        sideEffectsCacheKey: cacheKey,
        sideEffects: {
          remoteOrigin: {
            channel: pnprServer,
            signerKeyId: 'acme-2026',
            verification: 'verified',
          },
        },
      })
      expect(requestedPaths).toEqual([
        '/-/pnpr',
        '/-/pnpr/v0/artifacts/resolve',
        '/-/pnpr/v0/artifacts/blob',
      ])

      // Content the store already holds is the same content, addressed by the
      // digest the manifest carries, so a second restore transfers nothing.
      alreadyInStore.set(
        `${createHash('sha512').update(builtFile).digest('hex')}\0${0o755}`,
        alreadyStoredFile
      )
      storedFiles.length = 0
      requestedPaths.length = 0
      const reuse = createRemoteSideEffectsRestorer({
        allowBuild: candidate => candidate === depPath,
        configByUri: {},
        depsGraph,
        depsStateCache: {},
        ignoreScripts: false,
        pnprServer,
        settings: { organization: 'acme', packages: [packageName], trustedKeys },
        sideEffectsCacheRead: false,
        storeController,
      })
      const reusedFiles: PackageFilesResponse = {
        filesMap: new Map(),
        requiresBuild: true,
        resolvedFrom: 'remote',
      }
      const reusedKey = await reuse?.restore({
        graphKey,
        depPath,
        files: reusedFiles,
        name: packageName,
        resolution: { integrity: sourceIntegrity } as LockfileResolution,
        version: packageVersion,
      })
      expect(reusedFiles.sideEffectsMaps?.get(reusedKey!)?.added?.get('build/addon.node'))
        .toBe(alreadyStoredFile)
      expect(storedFiles).toEqual([])
      expect(requestedPaths).not.toContain('/-/pnpr/v0/artifacts/blob')

      requestedPaths.length = 0
      const persistedFiles: PackageFilesResponse = {
        filesMap: new Map(),
        requiresBuild: true,
        resolvedFrom: 'store',
        sideEffectsMaps: new Map([[cacheKey!, {
          added: new Map([
            ['build/addon.node', alreadyStoredFile],
            ['build/addon-copy.node', alreadyStoredFile],
          ]),
          deleted: ['src/intermediate.o'],
        }]]),
        sideEffectsDiffs: new Map([[cacheKey!, persisted[0].sideEffects]]),
      }
      const offlineReuse = createRemoteSideEffectsRestorer({
        allowBuild: candidate => candidate === depPath,
        configByUri: {},
        depsGraph,
        depsStateCache: {},
        ignoreScripts: false,
        settings: { organization: 'acme', packages: [packageName], trustedKeys },
        sideEffectsCacheRead: false,
        storeController,
      })
      await expect(offlineReuse?.restore({
        graphKey,
        depPath,
        files: persistedFiles,
        name: packageName,
        resolution: { integrity: sourceIntegrity } as LockfileResolution,
        version: packageVersion,
      })).resolves.toBe(cacheKey)
      expect(requestedPaths).toEqual([])

      requestedPaths.length = 0
      const changedChannelFiles: PackageFilesResponse = {
        ...persistedFiles,
        sideEffectsMaps: new Map(persistedFiles.sideEffectsMaps),
        sideEffectsDiffs: new Map(persistedFiles.sideEffectsDiffs),
      }
      const changedChannel = createRemoteSideEffectsRestorer({
        allowBuild: candidate => candidate === depPath,
        configByUri: {},
        depsGraph,
        depsStateCache: {},
        ignoreScripts: false,
        pnprServer: `${pnprServer}/other`,
        settings: { organization: 'acme', packages: [packageName], trustedKeys },
        sideEffectsCacheRead: true,
        storeController,
      })
      await expect(changedChannel?.restore({
        graphKey,
        depPath,
        files: changedChannelFiles,
        name: packageName,
        resolution: { integrity: sourceIntegrity } as LockfileResolution,
        version: packageVersion,
      })).resolves.toBeUndefined()
      expect(changedChannelFiles.sideEffectsMaps?.has(cacheKey!)).toBe(false)
      expect(requestedPaths).toEqual(['/other/-/pnpr'])

      const rejectedPersistedFiles: PackageFilesResponse = {
        ...persistedFiles,
        sideEffectsMaps: new Map(persistedFiles.sideEffectsMaps),
        sideEffectsDiffs: new Map(persistedFiles.sideEffectsDiffs),
      }
      const changedTrust = createRemoteSideEffectsRestorer({
        allowBuild: candidate => candidate === depPath,
        configByUri: {},
        depsGraph,
        depsStateCache: {},
        ignoreScripts: false,
        settings: { organization: 'acme', packages: [packageName], trustedKeys: { replacement: trustedKeys['acme-2026'] } },
        sideEffectsCacheRead: true,
        storeController,
      })
      await expect(changedTrust?.restore({
        graphKey,
        depPath,
        files: rejectedPersistedFiles,
        name: packageName,
        resolution: { integrity: sourceIntegrity } as LockfileResolution,
        version: packageVersion,
      })).resolves.toBeUndefined()
      expect(rejectedPersistedFiles.sideEffectsMaps?.has(cacheKey!)).toBe(false)

      const platformFingerprint = Object.keys(owners['organization:acme'])[0]
      const unrelatedPinLockfile: LockfileObject = {
        lockfileVersion: '9.0',
        importers: {},
        packages: {
          [depPath]: { resolution: { integrity: sourceIntegrity } },
          ['stale-package@1.0.0' as DepPath]: {
            artifactPins: {
              [inputKey]: {
                'organization:acme': { [platformFingerprint]: '0'.repeat(64) },
              },
            },
            resolution: { integrity: sourceIntegrity },
          },
        },
      }
      const snapshotScoped = createRemoteSideEffectsRestorer({
        allowBuild: candidate => candidate === depPath,
        artifactPinsLockfile: unrelatedPinLockfile,
        configByUri: {},
        depsGraph,
        depsStateCache: {},
        ignoreScripts: false,
        pnprServer,
        settings: { organization: 'acme', packages: [packageName], trustedKeys },
        sideEffectsCacheRead: false,
        storeController,
      })
      const snapshotScopedFiles: PackageFilesResponse = {
        filesMap: new Map(),
        requiresBuild: true,
        resolvedFrom: 'remote',
      }
      const snapshotScopedKey = await snapshotScoped?.restore({
        graphKey,
        depPath,
        files: snapshotScopedFiles,
        name: packageName,
        resolution: { integrity: sourceIntegrity } as LockfileResolution,
        version: packageVersion,
      })
      expect(snapshotScopedKey).toBeDefined()

      let releaseLatePinResolve!: () => void
      const waitForLatePinRelease = new Promise<void>((resolve) => {
        releaseLatePinResolve = resolve
      })
      let notifyLatePinResolveStarted!: () => void
      const latePinResolveStarted = new Promise<void>((resolve) => {
        notifyLatePinResolveStarted = resolve
      })
      heldResolve = { wait: waitForLatePinRelease, notifyStarted: notifyLatePinResolveStarted }
      const latePinnedDepPath = `${depPath}(peer@2.0.0)` as DepPath
      const latePinsLockfile: LockfileObject = {
        lockfileVersion: '9.0',
        importers: {},
        packages: {
          [depPath]: { resolution: { integrity: sourceIntegrity } },
          [latePinnedDepPath]: {
            artifactPins: {
              [inputKey]: {
                'organization:acme': { [platformFingerprint]: '0'.repeat(64) },
              },
            },
            resolution: { integrity: sourceIntegrity },
          },
        },
      }
      const latePinRestorer = createRemoteSideEffectsRestorer({
        allowBuild: candidate => candidate === depPath || candidate === latePinnedDepPath,
        artifactPinsLockfile: latePinsLockfile,
        configByUri: {},
        depsGraph,
        depsStateCache: {},
        ignoreScripts: false,
        pnprServer,
        settings: { organization: 'acme', packages: [packageName], trustedKeys },
        sideEffectsCacheRead: false,
        storeController,
      })!
      const unpinnedFiles: PackageFilesResponse = {
        filesMap: new Map(),
        requiresBuild: true,
        resolvedFrom: 'remote',
      }
      const unpinnedRestore = latePinRestorer.restore({
        graphKey,
        depPath,
        files: unpinnedFiles,
        name: packageName,
        resolution: { integrity: sourceIntegrity } as LockfileResolution,
        version: packageVersion,
      })
      await latePinResolveStarted
      const latePinnedFiles: PackageFilesResponse = {
        filesMap: new Map(),
        requiresBuild: true,
        resolvedFrom: 'remote',
      }
      const latePinnedRestore = latePinRestorer.restore({
        graphKey,
        depPath: latePinnedDepPath,
        files: latePinnedFiles,
        filesIndexFile: 'late-pin-row',
        name: packageName,
        resolution: { integrity: sourceIntegrity } as LockfileResolution,
        version: packageVersion,
      })
      releaseLatePinResolve()
      await expect(unpinnedRestore).resolves.toBeDefined()
      await expect(latePinnedRestore).resolves.toBeUndefined()
      expect(latePinnedFiles.sideEffectsMaps).toBeUndefined()
      expect(persisted.some(({ filesIndexFile }) => filesIndexFile === 'late-pin-row')).toBe(false)

      let releaseResolve!: () => void
      const waitForRelease = new Promise<void>((resolve) => {
        releaseResolve = resolve
      })
      let notifyResolveStarted!: () => void
      const resolveStarted = new Promise<void>((resolve) => {
        notifyResolveStarted = resolve
      })
      heldResolve = { wait: waitForRelease, notifyStarted: notifyResolveStarted }
      const conflictingDepPath = `${depPath}(peer@1.0.0)` as DepPath
      const envelopeDigest = owners['organization:acme'][platformFingerprint]
      const conflictingPinsLockfile: LockfileObject = {
        lockfileVersion: '9.0',
        importers: {},
        packages: {
          [depPath]: {
            artifactPins: {
              [inputKey]: {
                'organization:acme': { [platformFingerprint]: envelopeDigest },
              },
            },
            resolution: { integrity: sourceIntegrity },
          },
          [conflictingDepPath]: {
            artifactPins: {
              [inputKey]: {
                'organization:acme': { [platformFingerprint]: '0'.repeat(64) },
              },
            },
            resolution: { integrity: sourceIntegrity },
          },
        },
      }
      const collisionRestorer = createRemoteSideEffectsRestorer({
        allowBuild: candidate => candidate === depPath || candidate === conflictingDepPath,
        artifactPinsLockfile: conflictingPinsLockfile,
        configByUri: {},
        depsGraph,
        depsStateCache: {},
        ignoreScripts: false,
        pnprServer,
        settings: { organization: 'acme', packages: [packageName], trustedKeys },
        sideEffectsCacheRead: false,
        storeController,
      })!
      const firstCollisionFiles: PackageFilesResponse = {
        filesMap: new Map(),
        requiresBuild: true,
        resolvedFrom: 'remote',
      }
      const firstCollisionRestore = collisionRestorer.restore({
        graphKey,
        depPath,
        files: firstCollisionFiles,
        name: packageName,
        resolution: { integrity: sourceIntegrity } as LockfileResolution,
        version: packageVersion,
      })
      await resolveStarted
      const secondCollisionFiles: PackageFilesResponse = {
        filesMap: new Map(),
        requiresBuild: true,
        resolvedFrom: 'remote',
      }
      await expect(collisionRestorer.restore({
        graphKey,
        depPath: conflictingDepPath,
        files: secondCollisionFiles,
        name: packageName,
        resolution: { integrity: sourceIntegrity } as LockfileResolution,
        version: packageVersion,
      })).resolves.toBeUndefined()
      releaseResolve()
      await expect(firstCollisionRestore).resolves.toBeUndefined()
      expect(firstCollisionFiles.sideEffectsMaps).toBeUndefined()

      // Restoring is per package so one package never waits on another's
      // fetch, but packages restored together still share a lookup request.
      const secondGraphKey = `${graphKey}-second`
      const secondFiles: PackageFilesResponse = {
        filesMap: new Map(),
        requiresBuild: true,
        resolvedFrom: 'remote',
      }
      const batching = createRemoteSideEffectsRestorer<string>({
        allowBuild: () => true,
        configByUri: {},
        depsGraph: {
          ...depsGraph,
          [secondGraphKey]: { children: {}, fullPkgId: secondGraphKey },
        } as DepsGraph<string>,
        depsStateCache: {},
        ignoreScripts: false,
        pnprServer,
        settings: {
          organization: 'acme',
          packages: [packageName, `${packageName}-second`],
          trustedKeys,
        },
        sideEffectsCacheRead: false,
        storeController,
      })
      requestedPaths.length = 0
      const first = batching?.restore({
        graphKey,
        depPath,
        files: { ...files, sideEffectsMaps: undefined },
        name: packageName,
        resolution: { integrity: sourceIntegrity } as LockfileResolution,
        version: packageVersion,
      })
      // Arrive a tick apart, as two packages whose fetches land at slightly
      // different times do: only the batching window can still join them.
      await delay(5)
      const second = batching?.restore({
        graphKey: secondGraphKey,
        depPath: `${depPath}-second` as DepPath,
        files: secondFiles,
        name: `${packageName}-second`,
        resolution: { integrity: sourceIntegrity } as LockfileResolution,
        version: packageVersion,
      })
      const restored = await Promise.all([first, second])
      expect(restored.every((key) => key != null)).toBe(true)
      expect(requestedPaths.filter((path) => path === '/-/pnpr/v0/artifacts/resolve')).toHaveLength(1)

      alreadyInStore.clear()
      serverState.corruptBlob = true
      requestedPaths.length = 0
      const corruptRestorer = createRemoteSideEffectsRestorer({
        allowBuild: () => true,
        configByUri: {},
        depsGraph,
        depsStateCache: {},
        ignoreScripts: false,
        pnprServer,
        settings: { organization: 'acme', packages: [packageName], trustedKeys },
        sideEffectsCacheRead: false,
        storeController,
      })!
      await expect(corruptRestorer.restore({
        graphKey,
        depPath,
        files: { filesMap: new Map(), requiresBuild: true, resolvedFrom: 'remote' },
        filesIndexFile: 'corrupt-row',
        name: packageName,
        resolution: { integrity: sourceIntegrity } as LockfileResolution,
        version: packageVersion,
      })).resolves.toBeUndefined()
      expect(quarantined).toEqual([{
        channel: pnprServer,
        envelopeDigest: expect.stringMatching(/^[a-f0-9]{64}$/),
        filesIndexFile: 'corrupt-row',
      }])

      await expect(corruptRestorer.restore({
        graphKey,
        depPath,
        files: { filesMap: new Map(), requiresBuild: true, resolvedFrom: 'remote' },
        filesIndexFile: 'late-corrupt-row',
        name: packageName,
        resolution: { integrity: sourceIntegrity } as LockfileResolution,
        version: packageVersion,
      })).resolves.toBeUndefined()
      expect(quarantined).toEqual([
        {
          channel: pnprServer,
          envelopeDigest: expect.stringMatching(/^[a-f0-9]{64}$/),
          filesIndexFile: 'corrupt-row',
        },
        {
          channel: pnprServer,
          envelopeDigest: quarantined[0].envelopeDigest,
          filesIndexFile: 'late-corrupt-row',
        },
      ])

      requestedPaths.length = 0
      const quarantinedRestorer = createRemoteSideEffectsRestorer({
        allowBuild: () => true,
        configByUri: {},
        depsGraph,
        depsStateCache: {},
        ignoreScripts: false,
        pnprServer,
        settings: { organization: 'acme', packages: [packageName], trustedKeys },
        sideEffectsCacheRead: false,
        storeController,
      })!
      await expect(quarantinedRestorer.restore({
        graphKey,
        depPath,
        files: {
          filesMap: new Map(),
          requiresBuild: true,
          remoteSideEffectsQuarantine: new Map([[pnprServer, [quarantined[0].envelopeDigest]]]),
          resolvedFrom: 'store',
        },
        filesIndexFile: 'corrupt-row',
        name: packageName,
        resolution: { integrity: sourceIntegrity } as LockfileResolution,
        version: packageVersion,
      })).resolves.toBeUndefined()
      expect(requestedPaths).not.toContain('/-/pnpr/v0/artifacts/blob')
    } finally {
      await new Promise<void>((resolve, reject) => server.close(error => error == null ? resolve() : reject(error)))
    }
  })

  test('does not contact pnpr when build policy denies the package', async () => {
    const files: PackageFilesResponse = {
      filesMap: new Map(),
      requiresBuild: true,
      resolvedFrom: 'remote',
    }
    const restorer = createRemoteSideEffectsRestorer({
      allowBuild: () => false,
      configByUri: {},
      depsGraph: {
        [graphKey]: { children: {}, fullPkgId: graphKey },
      },
      depsStateCache: {},
      ignoreScripts: false,
      pnprServer: 'file:///must-not-be-opened',
      settings: {
        organization: 'acme',
        packages: [packageName],
        trustedKeys: { unused: 'AA==' },
      },
      sideEffectsCacheRead: false,
      storeController: { addFileToStore: () => {
        throw new Error('unexpected')
      } } as unknown as StoreController,
    })
    // A platform the PoC does not support has no restorer at all, which is the
    // same answer this test is about: nothing is asked of pnpr.
    await expect(Promise.resolve(restorer?.restore({
      graphKey,
      depPath,
      files,
      name: packageName,
      resolution: { integrity: sourceIntegrity } as LockfileResolution,
      version: packageVersion,
    }))).resolves.toBeUndefined()
  })

  test('publishes a signed build diff produced by install', async () => {
    if (currentLinuxGlibcPlatform() == null) return

    const { privateKey, publicKey } = generateKeyPairSync('ec', { namedCurve: 'prime256v1' })
    let publishedBody: Buffer | undefined
    const server = createServer((request, response) => {
      const chunks: Buffer[] = []
      request.on('data', chunk => chunks.push(Buffer.from(chunk)))
      request.on('end', () => {
        if (request.method === 'PUT' && request.url === '/-/pnpr/v0/artifacts') {
          publishedBody = Buffer.concat(chunks)
          response.writeHead(201).end()
        } else {
          response.writeHead(404).end()
        }
      })
    })
    const pnprServer = await listen(server)
    const temporaryDirectory = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-shared-side-effects-'))
    const builtFilePath = path.join(temporaryDirectory, 'addon.node')
    await fs.writeFile(builtFilePath, builtFile)
    try {
      await publishBuiltSharedSideEffects({
        configByUri: {},
        depsGraph: {
          [graphKey]: { children: {}, fullPkgId: graphKey },
        },
        graphKey,
        name: packageName,
        pnprServer,
        resolution: { integrity: sourceIntegrity } as LockfileResolution,
        settings: {
          organization: 'acme',
          packages: [packageName],
          publish: true,
          keyId: 'acme-2026',
          privateKey: privateKey.export({ format: 'der', type: 'pkcs8' }).toString('base64'),
          builderId: 'ci/main/42',
        },
        upload: {
          filesMap: new Map([['build/addon.node', builtFilePath]]),
          sideEffects: {
            added: new Map([['build/addon.node', {
              checkedAt: Date.now(),
              digest: createHash('sha512').update(builtFile).digest('hex'),
              mode: 0o755,
              size: builtFile.byteLength,
            }]]),
            deleted: ['src/intermediate.o'],
          },
        },
        version: packageVersion,
      })

      const published = JSON.parse(publishedBody!.toString('utf8')) as {
        blobs: Array<{ integrity: string, data: string }>
        envelope: SignedArtifactEnvelope
        key: string
      }
      const payload = verifySignedArtifactEnvelope(
        published.envelope,
        publicKey.export({ format: 'der', type: 'spki' }).toString('base64')
      )
      expect(payload.inputKey).toBe(published.key)
      expect(payload.package).toEqual({ name: packageName, version: packageVersion })
      expect(payload.sourceIntegrity).toBe(sourceIntegrity)
      expect(payload.manifest).toEqual({
        added: [{
          path: 'build/addon.node',
          integrity: builtFileIntegrity,
          mode: 0o755,
          size: builtFile.byteLength,
        }],
        deleted: ['src/intermediate.o'],
      })
      expect(published.blobs).toEqual([{
        integrity: builtFileIntegrity,
        data: builtFile.toString('base64'),
      }])
    } finally {
      await fs.rm(temporaryDirectory, { force: true, recursive: true })
      await new Promise<void>((resolve, reject) => server.close(error => error == null ? resolve() : reject(error)))
    }
  })
})

function currentLinuxGlibcPlatform (): LinuxGlibcPlatform | undefined {
  if (process.platform !== 'linux' || !['x64', 'arm64'].includes(process.arch)) return undefined
  const report = process.report?.getReport() as { header?: { glibcVersionRuntime?: string } }
  const [glibcMajor, glibcMinor] = report.header?.glibcVersionRuntime?.split('.').map(Number) ?? []
  const nodeMajor = Number(process.versions.node.split('.')[0])
  if (![nodeMajor, glibcMajor, glibcMinor].every(Number.isSafeInteger)) return undefined
  return { architecture: process.arch, nodeMajor, glibcMajor, glibcMinor }
}

async function listen (server: ReturnType<typeof createServer>): Promise<string> {
  await new Promise<void>((resolve, reject) => {
    server.once('error', reject)
    server.listen(0, '127.0.0.1', resolve)
  })
  const address = server.address()
  if (address == null || typeof address === 'string') throw new Error('Expected a TCP test server address')
  return `http://127.0.0.1:${address.port}`
}
