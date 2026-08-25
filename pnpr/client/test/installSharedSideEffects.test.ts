import { createHash, generateKeyPairSync } from 'node:crypto'
import fs from 'node:fs/promises'
import { createServer } from 'node:http'
import os from 'node:os'
import path from 'node:path'

import { describe, expect, test } from '@jest/globals'
import type { DepsGraph } from '@pnpm/deps.graph-hasher'
import type { LockfileResolution } from '@pnpm/lockfile.types'
import {
  applySharedSideEffectsToInstall,
  type ArtifactPayload,
  createSignedArtifactEnvelope,
  linuxGlibcCompatibilityTag,
  type LinuxGlibcPlatform,
  publishBuiltSharedSideEffects,
  type SignedArtifactEnvelope,
  verifySignedArtifactEnvelope,
} from '@pnpm/pnpr.client'
import type { PackageFilesResponse, StoreController } from '@pnpm/store.controller-types'
import type { DepPath } from '@pnpm/types'

const packageName = 'native-addon'
const packageVersion = '1.0.0'
const graphKey = `${packageName}@${packageVersion}`
const depPath = graphKey as DepPath
const sourceIntegrity = `sha512-${createHash('sha512').update('source').digest('base64')}`
const builtFile = Buffer.from('compiled native addon')
const builtFileIntegrity = `sha512-${createHash('sha512').update(builtFile).digest('base64')}`

describe('install shared side-effects', () => {
  test('hydrates the store and selects a verified remote build', async () => {
    const platform = currentLinuxGlibcPlatform()
    if (platform == null) return

    const { privateKey, publicKey } = generateKeyPairSync('ec', { namedCurve: 'prime256v1' })
    const requestedPaths: string[] = []
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
          const candidate = body.candidates[0]
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
          response.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify({
            artifacts: [{
              key: candidate.key,
              variants: [{
                envelope: createSignedArtifactEnvelope(payload, {
                  keyId: 'acme-2026',
                  privateKey,
                }),
              }],
            }],
          }))
          return
        }
        if (request.url === '/-/pnpr/v0/artifacts/blob') {
          response.writeHead(200, { 'content-type': 'application/octet-stream' }).end(builtFile)
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
    const storeController = {
      addFileToStore: (bytes: Buffer, mode: number) => {
        storedFiles.push({ bytes, mode })
        return {
          checkedAt: Date.now(),
          digest: createHash('sha512').update(bytes).digest('hex'),
          filePath: '/store/cafs/build-addon.node',
        }
      },
    } as unknown as StoreController
    const depsGraph: DepsGraph<typeof graphKey> = {
      [graphKey]: {
        children: {},
        fullPkgId: graphKey,
      },
    }

    try {
      const hits = await applySharedSideEffectsToInstall({
        allowBuild: candidate => candidate === depPath,
        configByUri: {},
        depsGraph,
        depsStateCache: {},
        ignoreScripts: false,
        nodes: [{
          graphKey,
          depPath,
          files,
          name: packageName,
          resolution: { integrity: sourceIntegrity } as LockfileResolution,
          version: packageVersion,
        }],
        pnprServer,
        settings: {
          organization: 'acme',
          packages: [packageName],
          trustedKeys: {
            'acme-2026': publicKey.export({ format: 'der', type: 'spki' }).toString('base64'),
          },
        },
        sideEffectsCacheRead: false,
        storeController,
      })

      const cacheKey = hits.get(graphKey)
      expect(cacheKey).toBeDefined()
      expect(files.sideEffectsMaps?.get(cacheKey!)).toEqual({
        added: new Map([
          ['build/addon.node', '/store/cafs/build-addon.node'],
          ['build/addon-copy.node', '/store/cafs/build-addon.node'],
        ]),
        deleted: ['src/intermediate.o'],
      })
      expect(storedFiles).toEqual([{ bytes: builtFile, mode: 0o755 }])
      expect(requestedPaths).toEqual([
        '/-/pnpr',
        '/-/pnpr/v0/artifacts/resolve',
        '/-/pnpr/v0/artifacts/blob',
      ])
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
    await expect(applySharedSideEffectsToInstall({
      allowBuild: () => false,
      configByUri: {},
      depsGraph: {
        [graphKey]: { children: {}, fullPkgId: graphKey },
      },
      depsStateCache: {},
      ignoreScripts: false,
      nodes: [{
        graphKey,
        depPath,
        files,
        name: packageName,
        resolution: { integrity: sourceIntegrity } as LockfileResolution,
        version: packageVersion,
      }],
      pnprServer: 'file:///must-not-be-opened',
      settings: {
        organization: 'acme',
        packages: [packageName],
        trustedKeys: {},
      },
      sideEffectsCacheRead: false,
      storeController: { addFileToStore: () => {
        throw new Error('unexpected')
      } } as unknown as StoreController,
    })).resolves.toEqual(new Map())
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
    const environmentKeys = [
      'PNPM_SHARED_SIDE_EFFECTS_CACHE_PUBLISH',
      'PNPM_SHARED_SIDE_EFFECTS_CACHE_KEY_ID',
      'PNPM_SHARED_SIDE_EFFECTS_CACHE_PRIVATE_KEY',
      'PNPM_SHARED_SIDE_EFFECTS_CACHE_BUILDER_ID',
    ] as const
    const originalEnvironment = Object.fromEntries(environmentKeys.map(key => [key, process.env[key]]))
    process.env.PNPM_SHARED_SIDE_EFFECTS_CACHE_PUBLISH = 'true'
    process.env.PNPM_SHARED_SIDE_EFFECTS_CACHE_KEY_ID = 'acme-2026'
    process.env.PNPM_SHARED_SIDE_EFFECTS_CACHE_PRIVATE_KEY = privateKey
      .export({ format: 'der', type: 'pkcs8' })
      .toString('base64')
    process.env.PNPM_SHARED_SIDE_EFFECTS_CACHE_BUILDER_ID = 'ci/main/42'

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
          trustedKeys: {},
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
      for (const key of environmentKeys) {
        const value = originalEnvironment[key]
        if (value == null) delete process.env[key]
        else process.env[key] = value
      }
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
