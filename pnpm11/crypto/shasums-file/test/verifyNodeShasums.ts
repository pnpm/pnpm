import { expect, test } from '@jest/globals'
import { fetchVerifiedNodeShasums } from '@pnpm/crypto.shasums-file'
import * as openpgp from 'openpgp'

const SHASUMS_URL = 'https://nodejs.example.test/download/release/v22.11.0/SHASUMS256.txt'
const SHASUMS = 'deadbeef'.repeat(8) + '  node-v22.11.0-darwin-arm64.tar.gz\n'

async function makeKey () {
  const { privateKey, publicKey } = await openpgp.generateKey({
    userIDs: [{ name: 'Test Node Releaser', email: 'test@nodejs.example' }],
    format: 'armored',
  })
  return { privateKey: await openpgp.readPrivateKey({ armoredKey: privateKey }), armoredKey: publicKey }
}

async function detachedSig (privateKey: openpgp.PrivateKey, content: string): Promise<Uint8Array> {
  const message = await openpgp.createMessage({ binary: new TextEncoder().encode(content) })
  return openpgp.sign({ message, signingKeys: privateKey, detached: true, format: 'binary' }) as Promise<Uint8Array>
}

function mockFetch (responses: Record<string, { ok: boolean, body?: Uint8Array }>) {
  return (async (url: string) => {
    const res = responses[url]
    if (!res) return { ok: false, status: 404 }
    return { ok: res.ok, status: res.ok ? 200 : 404, arrayBuffer: async () => res.body!.buffer }
  }) as never
}

test('returns the SHASUMS content when the detached signature verifies against a trusted key', async () => {
  const key = await makeKey()
  const fetch = mockFetch({
    [SHASUMS_URL]: { ok: true, body: new TextEncoder().encode(SHASUMS) },
    [`${SHASUMS_URL}.sig`]: { ok: true, body: await detachedSig(key.privateKey, SHASUMS) },
  })

  await expect(fetchVerifiedNodeShasums(fetch, SHASUMS_URL, [key])).resolves.toBe(SHASUMS)
})

test('throws when the signature was made by an untrusted key', async () => {
  const signer = await makeKey()
  const trusted = await makeKey()
  const fetch = mockFetch({
    [SHASUMS_URL]: { ok: true, body: new TextEncoder().encode(SHASUMS) },
    [`${SHASUMS_URL}.sig`]: { ok: true, body: await detachedSig(signer.privateKey, SHASUMS) },
  })

  await expect(fetchVerifiedNodeShasums(fetch, SHASUMS_URL, [trusted])).rejects.toThrow(/signature/i)
})

test('throws when the SHASUMS content was tampered with after signing', async () => {
  const key = await makeKey()
  const fetch = mockFetch({
    [SHASUMS_URL]: { ok: true, body: new TextEncoder().encode(SHASUMS.replace('deadbeef', 'tampered0')) },
    [`${SHASUMS_URL}.sig`]: { ok: true, body: await detachedSig(key.privateKey, SHASUMS) },
  })

  await expect(fetchVerifiedNodeShasums(fetch, SHASUMS_URL, [key])).rejects.toThrow(/signature/i)
})

test('throws when the signature file is missing', async () => {
  const key = await makeKey()
  const fetch = mockFetch({
    [SHASUMS_URL]: { ok: true, body: new TextEncoder().encode(SHASUMS) },
  })

  await expect(fetchVerifiedNodeShasums(fetch, SHASUMS_URL, [key])).rejects.toThrow(/SHASUMS256.txt.sig/)
})
