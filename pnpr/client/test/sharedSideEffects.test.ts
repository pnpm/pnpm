import { createHash, generateKeyPairSync, sign } from 'node:crypto'
import { createServer } from 'node:http'

import { describe, expect, test } from '@jest/globals'
import {
  type ArtifactPayload,
  compatibilityRank,
  createSignedArtifactEnvelope,
  type DependencySideEffectsPayload,
  downloadSharedArtifactBlob,
  linuxGlibcCompatibilityTag,
  linuxGlibcSupportedTags,
  macOSCompatibilityTag,
  macOSSupportedTags,
  platformFingerprint,
  publishSharedSideEffects,
  resolveSharedSideEffects,
  signedArtifactEnvelopeDigest,
  verifySignedArtifactEnvelope,
  windowsCompatibilityTag,
  windowsSupportedTags,
  type WorkspaceTaskPayload,
} from '@pnpm/pnpr.client'

const contents = Buffer.from('native-addon')
const integrity = `sha512-${createHash('sha512').update(contents).digest('base64')}`

function linux (glibcMinor: number, architecture = 'x64') {
  return { architecture, nodeMajor: 22, glibcMajor: 2, glibcMinor }
}

function payload (): DependencySideEffectsPayload {
  return {
    kind: 'dependency-side-effects:v1',
    subject: {
      kind: 'dependency-side-effects',
      package: { name: 'native-addon', version: '1.0.0' },
      sourceIntegrity: 'sha512-source',
    },
    inputKey: 'dependency-side-effects:v1:deps=abc',
    owner: { type: 'organization', name: 'acme' },
    builderId: 'ci/main/42',
    builderProfile: {
      imageDigest: 'sha256:image',
      architectureBaseline: 'x86-64-v2',
      environment: { CFLAGS: '-O2' },
    },
    compatibility: { kind: 'tagged', tags: [linuxGlibcCompatibilityTag(linux(17))] },
    manifest: {
      added: [{ path: 'build/addon.node', integrity, mode: 0o755, size: contents.byteLength }],
      deleted: ['src/intermediate.o'],
    },
  }
}

describe('signed shared artifacts', () => {
  test('verifies the exact payload bytes with a configured independent key', () => {
    const { privateKey, publicKey } = generateKeyPairSync('ec', { namedCurve: 'prime256v1' })
    const envelope = createSignedArtifactEnvelope(payload(), {
      keyId: 'acme-2026',
      privateKey: privateKey.export({ format: 'pem', type: 'pkcs8' }),
    })
    const publicKeySpki = publicKey.export({ format: 'der', type: 'spki' }).toString('base64')
    expect(verifySignedArtifactEnvelope(envelope, publicKeySpki)).toEqual(payload())
    expect(() => verifySignedArtifactEnvelope({ ...envelope, payload: `${envelope.payload}=` }, publicKeySpki)).toThrow('not valid base64')

    envelope.payload = Buffer.from(JSON.stringify({ ...payload(), builderId: 'attacker' })).toString('base64')
    expect(() => verifySignedArtifactEnvelope(envelope, publicKeySpki)).toThrow('signature verification failed')
  })

  test('matches the canonical cross-stack envelope digest vector', () => {
    const envelope = {
      algorithm: 'ecdsa-p256-sha256' as const,
      keyId: 'acme-2026',
      payload: 'eyJraW5kIjoiZGVwZW5kZW5jeS1zaWRlLWVmZmVjdHM6djEiLCJzdWJqZWN0Ijp7ImtpbmQiOiJkZXBlbmRlbmN5LXNpZGUtZWZmZWN0cyIsInBhY2thZ2UiOnsibmFtZSI6Im5hdGl2ZS1hZGRvbiIsInZlcnNpb24iOiIxLjAuMCJ9LCJzb3VyY2VJbnRlZ3JpdHkiOiJzaGE1MTItc291cmNlIn0sImlucHV0S2V5IjoiZGVwZW5kZW5jeS1zaWRlLWVmZmVjdHM6djE6ZGVwcz1hYmMiLCJvd25lciI6eyJ0eXBlIjoib3JnYW5pemF0aW9uIiwibmFtZSI6ImFjbWUifSwiYnVpbGRlcklkIjoiY2kvbWFpbi80MiIsImJ1aWxkZXJQcm9maWxlIjp7ImltYWdlRGlnZXN0Ijoic2hhMjU2OmltYWdlIiwiYXJjaGl0ZWN0dXJlQmFzZWxpbmUiOiJ4ODYtNjQtdjIiLCJlbnZpcm9ubWVudCI6eyJDRkxBR1MiOiItTzIifX0sImNvbXBhdGliaWxpdHkiOnsia2luZCI6InRhZ2dlZCIsInRhZ3MiOlsicG5wbTp2MTpsaW51eC14NjQtbm9kZTIyLWdsaWJjMi4xNyJdfSwibWFuaWZlc3QiOnsiYWRkZWQiOlt7InBhdGgiOiJidWlsZC9hZGRvbi5ub2RlIiwiaW50ZWdyaXR5Ijoic2hhNTEyLUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBPT0iLCJtb2RlIjo0OTMsInNpemUiOjV9XSwiZGVsZXRlZCI6WyJzcmMvaW50ZXJtZWRpYXRlLm8iXX19',
      signature: Buffer.from([0x30, 0x06, 0x02, 0x01, 0x01, 0x02, 0x01, 0x01]).toString('base64'),
    }
    expect(signedArtifactEnvelopeDigest(envelope)).toBe('20b3fbc179563fc173c1bd306b8d088eb0eebb6fa40998e55d645c414f1964f5')
    expect(() => signedArtifactEnvelopeDigest({ ...envelope, payload: `${envelope.payload}=` })).toThrow('base64')
  })

  test('rejects signing and verification keys outside P-256', () => {
    const { privateKey, publicKey } = generateKeyPairSync('rsa', { modulusLength: 2_048 })
    expect(() => createSignedArtifactEnvelope(payload(), {
      keyId: 'acme-rsa',
      privateKey: privateKey.export({ format: 'pem', type: 'pkcs8' }),
    })).toThrow('P-256 EC private key')

    const { privateKey: p256PrivateKey } = generateKeyPairSync('ec', { namedCurve: 'prime256v1' })
    const envelope = createSignedArtifactEnvelope(payload(), {
      keyId: 'acme-2026',
      privateKey: p256PrivateKey.export({ format: 'pem', type: 'pkcs8' }),
    })
    const publicKeySpki = publicKey.export({ format: 'der', type: 'spki' }).toString('base64')
    expect(() => verifySignedArtifactEnvelope(envelope, publicKeySpki)).toThrow('P-256 EC public key')

    const { privateKey: p384PrivateKey, publicKey: p384PublicKey } = generateKeyPairSync('ec', { namedCurve: 'secp384r1' })
    expect(() => createSignedArtifactEnvelope(payload(), {
      keyId: 'acme-p384',
      privateKey: p384PrivateKey.export({ format: 'pem', type: 'pkcs8' }),
    })).toThrow('P-256 EC private key')
    expect(() => verifySignedArtifactEnvelope(
      envelope,
      p384PublicKey.export({ format: 'der', type: 'spki' }).toString('base64')
    )).toThrow('P-256 EC public key')
  })

  test.each([
    '/absolute',
    '../escape',
    'a/../escape',
    'a\\b',
    'C:/escape',
    'double//segment',
    'dot/./segment',
    'trailing-dot.',
    'trailing-space ',
    'dir/trailing-dot.',
    'dir/trailing-space ',
    'dir/addon.node:payload',
    'CON',
    'dir/NUL.txt',
    'dir/com1.js',
    'COM¹',
    'dir/LPT².txt',
    'dir/LpT9',
    'nul\0byte',
  ])('rejects unsafe manifest path %p before signing', (path) => {
    const { privateKey } = generateKeyPairSync('ec', { namedCurve: 'prime256v1' })
    const invalid = payload()
    invalid.manifest.added[0].path = path
    expect(() => createSignedArtifactEnvelope(invalid, {
      keyId: 'acme-2026',
      privateKey: privateKey.export({ format: 'pem', type: 'pkcs8' }),
    })).toThrow()
  })

  test('rejects inconsistent sizes for one content-addressed blob', () => {
    const { privateKey } = generateKeyPairSync('ec', { namedCurve: 'prime256v1' })
    const invalid = payload()
    invalid.manifest.added.push({
      path: 'build/addon-copy.node',
      integrity,
      mode: 0o755,
      size: contents.byteLength + 1,
    })
    expect(() => createSignedArtifactEnvelope(invalid, {
      keyId: 'acme-2026',
      privateKey: privateKey.export({ format: 'pem', type: 'pkcs8' }),
    })).toThrow('inconsistent sizes')
  })

  test('validates publication inputs before opening a connection', async () => {
    const { privateKey } = generateKeyPairSync('ec', { namedCurve: 'prime256v1' })
    const envelope = createSignedArtifactEnvelope(payload(), {
      keyId: 'acme-2026',
      privateKey: privateKey.export({ format: 'pem', type: 'pkcs8' }),
    })
    const base = {
      registryUrl: 'file:///must-not-be-opened',
      key: payload().inputKey,
      envelope,
    }
    await expect(publishSharedSideEffects({
      ...base,
      key: `${payload().inputKey}-other`,
      blobs: [],
    })).rejects.toThrow('does not match')
    await expect(publishSharedSideEffects({
      ...base,
      blobs: [
        { integrity, data: contents.toString('base64') },
        { integrity, data: contents.toString('base64') },
      ],
    })).rejects.toThrow('Duplicate')
    const unrelated = createHash('sha512').update('unrelated').digest('base64')
    await expect(publishSharedSideEffects({
      ...base,
      blobs: [{ integrity: `sha512-${unrelated}`, data: Buffer.from('unrelated').toString('base64') }],
    })).rejects.toThrow('not referenced')
  })

  test('publishes workspace task artifacts with their subject-specific key prefix', async () => {
    const { privateKey } = generateKeyPairSync('ec', { namedCurve: 'prime256v1' })
    const taskPayload: WorkspaceTaskPayload = {
      ...payload(),
      kind: 'workspace-task:v1',
      subject: { kind: 'workspace-task', project: 'packages/app', task: 'build' },
      inputKey: 'workspace-task:v1:inputs=abc',
      compatibility: { kind: 'universal' },
      manifest: { added: [], deleted: [] },
    }
    const envelope = createSignedArtifactEnvelope(taskPayload, {
      keyId: 'acme-2026',
      privateKey: privateKey.export({ format: 'pem', type: 'pkcs8' }),
    })
    let publishedBody: Buffer | undefined
    const server = createServer((request, response) => {
      const chunks: Buffer[] = []
      request.on('data', chunk => chunks.push(Buffer.from(chunk)))
      request.on('end', () => {
        publishedBody = Buffer.concat(chunks)
        response.writeHead(201).end()
      })
    })
    const registryUrl = await listen(server)
    try {
      await publishSharedSideEffects({
        registryUrl,
        key: taskPayload.inputKey,
        envelope,
        blobs: [],
      })
      expect(JSON.parse(publishedBody!.toString('utf8'))).toMatchObject({
        key: taskPayload.inputKey,
        envelope,
      })
    } finally {
      await new Promise<void>((resolve, reject) => server.close(error => error == null ? resolve() : reject(error)))
    }
  })

  test('uses canonical compatibility tags and platform fingerprints', () => {
    const supportedTags = linuxGlibcSupportedTags(linux(3))
    expect(supportedTags).toEqual([
      'pnpm:v1:linux-x64-node22-glibc2.3',
      'pnpm:v1:linux-x64-node22-glibc2.2',
      'pnpm:v1:linux-x64-node22-glibc2.1',
      'pnpm:v1:linux-x64-node22-glibc2.0',
    ])
    expect(platformFingerprint(supportedTags)).toBe('fdfaaed730a56031779ee5e572e1e82aad454501ec5fbcfad6648e8a1e465f0c')

    const macOSSupported = macOSSupportedTags(macOS(15, 5))
    expect(macOSSupported).toEqual(['pnpm:v1:darwin-arm64-node22-macos15.5'])
    expect(platformFingerprint(macOSSupported)).toBe('b56fa5629b56d18308bbf7978d61b9afaf862e133ad18aef31588e0888eef3f8')
    expect(compatibilityRank({
      kind: 'tagged',
      tags: [macOSCompatibilityTag(macOS(15, 4))],
    }, macOSSupported)).toBe(65)
    expect(compatibilityRank({
      kind: 'tagged',
      tags: [macOSCompatibilityTag(macOS(14, 6))],
    }, macOSSupported)).toBe(1_000_063)
    expect(compatibilityRank({
      kind: 'tagged',
      tags: [macOSCompatibilityTag(macOS(16, 0))],
    }, macOSSupported)).toBeUndefined()
    expect(compatibilityRank({ kind: 'universal' }, macOSSupported)).toBe(Number.MAX_SAFE_INTEGER)

    const multipleMacOSSupported = [
      macOSCompatibilityTag(macOS(15, 5)),
      macOSCompatibilityTag(macOS(14, 6)),
    ]
    expect(compatibilityRank({
      kind: 'tagged',
      tags: [macOSCompatibilityTag(macOS(14, 6))],
    }, multipleMacOSSupported)).toBe(1)
    expect(compatibilityRank({
      kind: 'tagged',
      tags: [macOSCompatibilityTag(macOS(15, 4))],
    }, multipleMacOSSupported)).toBe(65)

    const windowsSupported = windowsSupportedTags(windows({ major: 10, minor: 0, build: 26_100 }))
    expect(windowsSupported).toEqual(['pnpm:v1:win32-x64-node22-windows10.0.26100'])
    expect(platformFingerprint(windowsSupported)).toBe('f5590f12a6d651acdcb3b60d7d25a5d2e1ad2f5af3e53d841391dec9e871c46e')
    expect(compatibilityRank({
      kind: 'tagged',
      tags: [windowsCompatibilityTag(windows({ major: 10, minor: 0, build: 22_621 }))],
    }, windowsSupported)).toBe(3_543)
    expect(compatibilityRank({
      kind: 'tagged',
      tags: [windowsCompatibilityTag(windows({ major: 6, minor: 3, build: 9_600 }))],
    }, windowsSupported)).toBe(3_997_016_564)
    expect(compatibilityRank({
      kind: 'tagged',
      tags: [windowsCompatibilityTag(windows({ major: 10, minor: 0, build: 26_101 }))],
    }, windowsSupported)).toBeUndefined()
    expect(compatibilityRank({ kind: 'universal' }, windowsSupported)).toBe(Number.MAX_SAFE_INTEGER)

    for (const invalid of [
      'pnpm:v2:linux-x64-node22-glibc2.17',
      'pnpm:v1:darwin-x64-node22-glibc2.17',
      'pnpm:v1:darwin-x64-node22-macos15',
      'pnpm:v1:darwin-x64-node22-macos015.5',
      'pnpm:v1:win32-x64-node22-windows10.0',
      'pnpm:v1:win32-x64-node22-windows10.0.026100',
      'pnpm:v1:linux-x64-node022-glibc2.17',
      'pnpm:v1:linux-x64-node22-glibc02.17',
      'pnpm:v1:linux-x64-node22-glibc2',
    ]) {
      const invalidPayload = payload()
      invalidPayload.compatibility = { kind: 'tagged', tags: [invalid] }
      const { privateKey } = generateKeyPairSync('ec', { namedCurve: 'prime256v1' })
      expect(() => createSignedArtifactEnvelope(invalidPayload, {
        keyId: 'acme-2026',
        privateKey: privateKey.export({ format: 'pem', type: 'pkcs8' }),
      })).toThrow()
    }
  })

  test('requires publisher ownership to match the signed package', () => {
    const invalid = payload()
    invalid.owner = { type: 'publisher', package: 'another-package' }
    const { privateKey } = generateKeyPairSync('ec', { namedCurve: 'prime256v1' })
    expect(() => createSignedArtifactEnvelope(invalid, {
      keyId: 'publisher-2026',
      privateKey: privateKey.export({ format: 'pem', type: 'pkcs8' }),
    })).toThrow('does not match')
  })

  test('validates workspace task subjects against their kind, key, and owner', () => {
    const taskPayload: ArtifactPayload = {
      ...payload(),
      kind: 'workspace-task:v1',
      subject: { kind: 'workspace-task', project: 'packages/app', task: 'build' },
      inputKey: 'workspace-task:v1:inputs=abc',
    }
    const { privateKey } = generateKeyPairSync('ec', { namedCurve: 'prime256v1' })
    const signingOptions = {
      keyId: 'acme-2026',
      privateKey: privateKey.export({ format: 'pem', type: 'pkcs8' }),
    }
    expect(() => createSignedArtifactEnvelope(taskPayload, signingOptions)).not.toThrow()
    expect(() => createSignedArtifactEnvelope({
      ...taskPayload,
      kind: 'dependency-side-effects:v1',
    } as unknown as ArtifactPayload, signingOptions)).toThrow('artifact kind')
    expect(() => createSignedArtifactEnvelope({
      ...taskPayload,
      owner: { type: 'publisher', package: 'app' },
    }, signingOptions)).toThrow('organization owner')
  })

  test('does not contact the cache when scripts, eligibility, or allowBuild deny reuse', async () => {
    const candidate = {
      key: payload().inputKey,
      subject: payload().subject,
      owner: payload().owner,
    }
    const base = {
      registryUrl: 'file:///must-not-be-opened',
      candidates: [candidate],
      supportedTags: linuxGlibcSupportedTags(linux(17)),
      trustedKeys: {},
    }
    await expect(resolveSharedSideEffects({
      ...base,
      policy: {
        ignoreScripts: true,
        eligiblePackages: new Set([candidate.subject.package.name]),
        allowedBuilds: new Set([candidate.subject.package.name]),
      },
    })).resolves.toEqual(new Map())
    await expect(resolveSharedSideEffects({
      ...base,
      policy: {
        ignoreScripts: false,
        eligiblePackages: new Set(),
        allowedBuilds: new Set([candidate.subject.package.name]),
      },
    })).resolves.toEqual(new Map())
    await expect(resolveSharedSideEffects({
      ...base,
      policy: {
        ignoreScripts: false,
        eligiblePackages: new Set([candidate.subject.package.name]),
        allowedBuilds: new Set(),
      },
    })).resolves.toEqual(new Map())
  })

  test('publishes, selects, and downloads through the v0 HTTP protocol', async () => {
    const { privateKey, publicKey } = generateKeyPairSync('ec', { namedCurve: 'prime256v1' })
    const envelope = createSignedArtifactEnvelope(payload(), {
      keyId: 'acme-2026',
      privateKey: privateKey.export({ format: 'pem', type: 'pkcs8' }),
    })
    const alternateEnvelope = createSignedArtifactEnvelope(payload(), {
      keyId: 'acme-2026',
      privateKey: privateKey.export({ format: 'pem', type: 'pkcs8' }),
    })
    const malformedPayload = Buffer.from('{')
    const malformedEnvelope = {
      algorithm: 'ecdsa-p256-sha256' as const,
      keyId: 'acme-2026',
      payload: malformedPayload.toString('base64'),
      signature: sign('sha256', malformedPayload, privateKey).toString('base64'),
    }
    const variants = [envelope, alternateEnvelope]
      .sort((left, right) => signedArtifactEnvelopeDigest(right).localeCompare(signedArtifactEnvelopeDigest(left)))
    const requests: Array<{ url: string | undefined, authorization: string | undefined, body: Buffer }> = []
    const server = createServer((request, response) => {
      const chunks: Buffer[] = []
      request.on('data', chunk => chunks.push(Buffer.from(chunk)))
      request.on('end', () => {
        requests.push({
          url: request.url,
          authorization: request.headers.authorization,
          body: Buffer.concat(chunks),
        })
        if (request.url === '/-/pnpr/v0/artifacts') {
          response.writeHead(201).end()
        } else if (request.url === '/-/pnpr/v0/artifacts/resolve') {
          const body = JSON.stringify({
            artifacts: [{
              key: payload().inputKey,
              variants: [...variants, malformedEnvelope].map(envelope => ({ envelope })),
            }],
          })
          response.writeHead(200, { 'content-type': 'application/json' }).end(body)
        } else if (request.url === '/-/pnpr/v0/artifacts/blob') {
          response.writeHead(200, { 'content-type': 'application/octet-stream' }).end(contents)
        } else {
          response.writeHead(404).end()
        }
      })
    })
    const registryUrl = await listen(server)
    try {
      await publishSharedSideEffects({
        registryUrl,
        authorization: 'Bearer token',
        key: payload().inputKey,
        envelope,
        blobs: [{ integrity, data: contents.toString('base64') }],
      })
      const mismatchedPackage = await resolveSharedSideEffects({
        registryUrl,
        authorization: 'Bearer token',
        candidates: [{
          key: payload().inputKey,
          subject: {
            ...payload().subject,
            package: { ...payload().subject.package, version: '2.0.0' },
          },
          owner: payload().owner,
        }],
        supportedTags: linuxGlibcSupportedTags(linux(17)),
        policy: {
          ignoreScripts: false,
          eligiblePackages: new Set([payload().subject.package.name]),
          allowedBuilds: new Set([payload().subject.package.name]),
        },
        trustedKeys: {
          'acme-2026': publicKey.export({ format: 'der', type: 'spki' }).toString('base64'),
        },
      })
      expect(mismatchedPackage).toEqual(new Map())
      const resolveOptions = {
        registryUrl,
        authorization: 'Bearer token',
        candidates: [{
          key: payload().inputKey,
          subject: payload().subject,
          owner: payload().owner,
        }],
        supportedTags: linuxGlibcSupportedTags(linux(17)),
        policy: {
          ignoreScripts: false,
          eligiblePackages: new Set([payload().subject.package.name]),
          allowedBuilds: new Set([payload().subject.package.name]),
        },
        trustedKeys: {
          'acme-2026': publicKey.export({ format: 'der', type: 'spki' }).toString('base64'),
        },
      }
      const selected = await resolveSharedSideEffects(resolveOptions)
      expect(selected.get(payload().inputKey)?.payload).toEqual(payload())
      expect(selected.get(payload().inputKey)?.envelopeDigest).toBe(
        [envelope, alternateEnvelope]
          .map(signedArtifactEnvelopeDigest)
          .sort()[0]
      )
      const selectedDigest = selected.get(payload().inputKey)!.envelopeDigest
      const rejected: Array<{ inputKey: string, envelopeDigest: string, reason: string }> = []
      await resolveSharedSideEffects({
        ...resolveOptions,
        onRejectedArtifact: rejection => rejected.push(rejection),
      })
      expect(rejected).toHaveLength(1)
      expect(rejected[0]).toMatchObject({
        inputKey: payload().inputKey,
        envelopeDigest: signedArtifactEnvelopeDigest(malformedEnvelope),
      })
      const afterQuarantine = await resolveSharedSideEffects({
        ...resolveOptions,
        quarantinedEnvelopeDigests: new Map([[payload().inputKey, new Set([selectedDigest])]]),
      })
      expect(afterQuarantine.get(payload().inputKey)?.envelopeDigest).toBe(
        [envelope, alternateEnvelope]
          .map(signedArtifactEnvelopeDigest)
          .find(digest => digest !== selectedDigest)
      )
      await expect(downloadSharedArtifactBlob({
        registryUrl,
        authorization: 'Bearer token',
        request: { owner: payload().owner, integrity },
      })).resolves.toEqual(contents)
      expect(requests.map(request => request.url)).toEqual([
        '/-/pnpr/v0/artifacts',
        '/-/pnpr/v0/artifacts/resolve',
        '/-/pnpr/v0/artifacts/resolve',
        '/-/pnpr/v0/artifacts/resolve',
        '/-/pnpr/v0/artifacts/resolve',
        '/-/pnpr/v0/artifacts/blob',
      ])
      expect(requests.every(request => request.authorization === 'Bearer token')).toBe(true)
    } finally {
      await new Promise<void>((resolve, reject) => server.close(error => error == null ? resolve() : reject(error)))
    }
  })

  test('an over-limit content-length rejects instead of hanging until the request timeout', async () => {
    const server = createServer((request, response) => {
      request.resume()
      request.on('end', () => {
        // Claim far more than the blob endpoint's ceiling without sending it,
        // so only the declared length can reject the request.
        response.writeHead(200, {
          'content-type': 'application/octet-stream',
          'content-length': String(1024 ** 4),
        })
        response.flushHeaders()
      })
    })
    const registryUrl = await listen(server)
    try {
      await expect(downloadSharedArtifactBlob({
        registryUrl,
        request: { owner: payload().owner, integrity },
      })).rejects.toThrow(/exceeds/)
    } finally {
      server.closeAllConnections()
      await new Promise<void>((resolve, reject) => server.close(error => error == null ? resolve() : reject(error)))
    }
  })

  test('rejects an envelope whose encoded fields exceed their limits', () => {
    expect(() => verifySignedArtifactEnvelope({
      algorithm: 'ecdsa-p256-sha256',
      keyId: 'acme-2026',
      payload: 'A'.repeat(Math.ceil((2 * 1024 * 1024) / 3) * 4 + 1),
      signature: 'AAAAAAAA',
    }, 'unused')).toThrow(/payload exceeds/)
    expect(() => verifySignedArtifactEnvelope({
      algorithm: 'ecdsa-p256-sha256',
      keyId: 'acme-2026',
      payload: Buffer.from(JSON.stringify(payload())).toString('base64'),
      signature: 'A'.repeat(Math.ceil(72 / 3) * 4 + 1),
    }, 'unused')).toThrow(/canonical P-256 DER/)
  })
})

function macOS (macOSMajor: number, macOSMinor: number, architecture = 'arm64') {
  return { architecture, nodeMajor: 22, macOSMajor, macOSMinor }
}

function windows ({ major, minor, build, architecture = 'x64' }: {
  major: number
  minor: number
  build: number
  architecture?: string
}) {
  return {
    architecture,
    nodeMajor: 22,
    windowsMajor: major,
    windowsMinor: minor,
    windowsBuild: build,
  }
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
