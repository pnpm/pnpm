import { describe, expect, test } from '@jest/globals'

import { fetchAttestationPublishedAt } from '../src/fetchAttestationPublishedAt.js'

type StubFetch = (url: string, opts?: unknown) => Promise<{
  status: number
  json: () => Promise<unknown>
}>

function makeFetchOpts (fetch: StubFetch) {
  return {
    fetch: fetch as never,
    retry: { retries: 0 },
    timeout: 10_000,
    fetchWarnTimeoutMs: 10_000,
  }
}

const REGISTRY = 'https://registry.npmjs.org/'

function attestationResponse (...integratedTimes: Array<string | number>): unknown {
  return {
    attestations: integratedTimes.map((t) => ({
      predicateType: 'https://github.com/npm/attestation/tree/main/specs/publish/v0.1',
      bundle: {
        verificationMaterial: {
          tlogEntries: [{ integratedTime: t }],
        },
      },
    })),
  }
}

describe('fetchAttestationPublishedAt', () => {
  test('returns an ISO timestamp built from tlogEntries[].integratedTime', async () => {
    // 1778583836 (Unix seconds) → 2026-05-12T05:43:56.000Z
    const fetch: StubFetch = async () => ({
      status: 200,
      json: async () => attestationResponse('1778583836'),
    })
    const result = await fetchAttestationPublishedAt(
      makeFetchOpts(fetch),
      'pnpm',
      '11.1.1',
      { registry: REGISTRY }
    )
    expect(result).toBe(new Date(1778583836 * 1000).toISOString())
  })

  test('hits /-/npm/v1/attestations/<name>@<version> with a literal name@version spec', async () => {
    const seenUrls: string[] = []
    const fetch: StubFetch = async (url) => {
      seenUrls.push(url)
      return { status: 404, json: async () => null }
    }
    await fetchAttestationPublishedAt(makeFetchOpts(fetch), 'pnpm', '11.1.1', { registry: REGISTRY })
    expect(seenUrls).toEqual(['https://registry.npmjs.org/-/npm/v1/attestations/pnpm@11.1.1'])
  })

  test('scoped package name passes through unencoded (slashes are path separators)', async () => {
    const seenUrls: string[] = []
    const fetch: StubFetch = async (url) => {
      seenUrls.push(url)
      return { status: 404, json: async () => null }
    }
    await fetchAttestationPublishedAt(
      makeFetchOpts(fetch),
      '@pnpm/exe',
      '11.1.1',
      { registry: REGISTRY }
    )
    expect(seenUrls).toEqual(['https://registry.npmjs.org/-/npm/v1/attestations/@pnpm/exe@11.1.1'])
  })

  test('returns undefined when the registry has no attestations for the package (404)', async () => {
    const fetch: StubFetch = async () => ({ status: 404, json: async () => null })
    const result = await fetchAttestationPublishedAt(
      makeFetchOpts(fetch),
      'pnpm',
      '11.1.1',
      { registry: REGISTRY }
    )
    expect(result).toBeUndefined()
  })

  test('returns undefined on 5xx — caller falls back to full metadata', async () => {
    const fetch: StubFetch = async () => ({ status: 503, json: async () => null })
    const result = await fetchAttestationPublishedAt(
      makeFetchOpts(fetch),
      'pnpm',
      '11.1.1',
      { registry: REGISTRY }
    )
    expect(result).toBeUndefined()
  })

  test('returns undefined when the fetch itself throws (network error)', async () => {
    const fetch: StubFetch = async () => {
      throw new Error('ECONNREFUSED')
    }
    const result = await fetchAttestationPublishedAt(
      makeFetchOpts(fetch),
      'pnpm',
      '11.1.1',
      { registry: REGISTRY }
    )
    expect(result).toBeUndefined()
  })

  test('returns undefined when the body is malformed JSON', async () => {
    const fetch: StubFetch = async () => ({
      status: 200,
      json: async () => {
        throw new SyntaxError('bad')
      },
    })
    const result = await fetchAttestationPublishedAt(
      makeFetchOpts(fetch),
      'pnpm',
      '11.1.1',
      { registry: REGISTRY }
    )
    expect(result).toBeUndefined()
  })

  test('returns undefined when the response has no attestations array', async () => {
    const fetch: StubFetch = async () => ({ status: 200, json: async () => ({}) })
    const result = await fetchAttestationPublishedAt(
      makeFetchOpts(fetch),
      'pnpm',
      '11.1.1',
      { registry: REGISTRY }
    )
    expect(result).toBeUndefined()
  })

  test('returns undefined when no tlogEntry carries a usable integratedTime', async () => {
    const fetch: StubFetch = async () => ({
      status: 200,
      json: async () => ({
        attestations: [{ bundle: { verificationMaterial: { tlogEntries: [{}] } } }],
      }),
    })
    const result = await fetchAttestationPublishedAt(
      makeFetchOpts(fetch),
      'pnpm',
      '11.1.1',
      { registry: REGISTRY }
    )
    expect(result).toBeUndefined()
  })

  test('picks the earliest integratedTime across multiple attestations', async () => {
    // SLSA provenance is signed slightly before npm publish attestation;
    // earlier integratedTime is the conservative pick.
    const fetch: StubFetch = async () => ({
      status: 200,
      json: async () => attestationResponse('1778583836', '1778583833'),
    })
    const result = await fetchAttestationPublishedAt(
      makeFetchOpts(fetch),
      'pnpm',
      '11.1.1',
      { registry: REGISTRY }
    )
    expect(result).toBe(new Date(1778583833 * 1000).toISOString())
  })

  test('accepts integratedTime as a number too (defensive against schema drift)', async () => {
    const fetch: StubFetch = async () => ({
      status: 200,
      json: async () => attestationResponse(1778583836),
    })
    const result = await fetchAttestationPublishedAt(
      makeFetchOpts(fetch),
      'pnpm',
      '11.1.1',
      { registry: REGISTRY }
    )
    expect(result).toBe(new Date(1778583836 * 1000).toISOString())
  })

  test('forwards the auth header to the fetch call', async () => {
    let seenAuth: string | undefined
    const fetch: StubFetch = async (_url, opts) => {
      seenAuth = (opts as { authHeaderValue?: string } | undefined)?.authHeaderValue
      return { status: 404, json: async () => null }
    }
    await fetchAttestationPublishedAt(
      makeFetchOpts(fetch),
      'pnpm',
      '11.1.1',
      { registry: REGISTRY, authHeaderValue: 'Bearer secret' }
    )
    expect(seenAuth).toBe('Bearer secret')
  })

  test('strips a trailing slash on the registry URL', async () => {
    const seenUrls: string[] = []
    const fetch: StubFetch = async (url) => {
      seenUrls.push(url)
      return { status: 404, json: async () => null }
    }
    await fetchAttestationPublishedAt(
      makeFetchOpts(fetch),
      'pnpm',
      '11.1.1',
      { registry: 'https://registry.npmjs.org/' }
    )
    expect(seenUrls[0]).toBe('https://registry.npmjs.org/-/npm/v1/attestations/pnpm@11.1.1')
  })
})
