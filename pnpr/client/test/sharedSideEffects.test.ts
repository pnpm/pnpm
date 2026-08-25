import { createHash, generateKeyPairSync } from 'node:crypto'
import { createServer } from 'node:http'

import { describe, expect, test } from '@jest/globals'
import {
  type ArtifactPayload,
  createSignedArtifactEnvelope,
  downloadSharedArtifactBlob,
  publishSharedSideEffects,
  resolveSharedSideEffects,
  verifySignedArtifactEnvelope,
} from '@pnpm/pnpr.client'

const contents = Buffer.from('native-addon')
const integrity = `sha512-${createHash('sha512').update(contents).digest('base64')}`

function payload (): ArtifactPayload {
  return {
    kind: 'dependency-side-effects:v1',
    sourceIntegrity: 'sha512-source',
    inputKey: 'dependency-side-effects:v1:deps=abc',
    owner: { type: 'organization', name: 'acme' },
    builderId: 'ci/main/42',
    builderProfile: {
      imageDigest: 'sha256:image',
      architectureBaseline: 'x86-64-v2',
      environment: { CFLAGS: '-O2' },
    },
    compatibility: { kind: 'tagged', tags: ['pnpm:v1:linux-x64-node22-glibc'] },
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

  test('publishes, selects, and downloads through the v0 HTTP protocol', async () => {
    const { privateKey, publicKey } = generateKeyPairSync('ec', { namedCurve: 'prime256v1' })
    const envelope = createSignedArtifactEnvelope(payload(), {
      keyId: 'acme-2026',
      privateKey: privateKey.export({ format: 'pem', type: 'pkcs8' }),
    })
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
            artifacts: [{ key: payload().inputKey, variants: [{ envelope }] }],
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
      const selected = await resolveSharedSideEffects({
        registryUrl,
        authorization: 'Bearer token',
        candidates: [{
          key: payload().inputKey,
          sourceIntegrity: payload().sourceIntegrity,
          owner: payload().owner,
        }],
        supportedTags: ['pnpm:v1:linux-x64-node22-glibc'],
        trustedKeys: {
          'acme-2026': publicKey.export({ format: 'der', type: 'spki' }).toString('base64'),
        },
      })
      expect(selected.get(payload().inputKey)?.payload).toEqual(payload())
      await expect(downloadSharedArtifactBlob({
        registryUrl,
        authorization: 'Bearer token',
        request: { owner: payload().owner, integrity },
      })).resolves.toEqual(contents)
      expect(requests.map(request => request.url)).toEqual([
        '/-/pnpr/v0/artifacts',
        '/-/pnpr/v0/artifacts/resolve',
        '/-/pnpr/v0/artifacts/blob',
      ])
      expect(requests.every(request => request.authorization === 'Bearer token')).toBe(true)
    } finally {
      await new Promise<void>((resolve, reject) => server.close(error => error == null ? resolve() : reject(error)))
    }
  })
})

async function listen (server: ReturnType<typeof createServer>): Promise<string> {
  await new Promise<void>((resolve, reject) => {
    server.once('error', reject)
    server.listen(0, '127.0.0.1', resolve)
  })
  const address = server.address()
  if (address == null || typeof address === 'string') throw new Error('Expected a TCP test server address')
  return `http://127.0.0.1:${address.port}`
}
