import { expect, test } from '@jest/globals'
import type { PackageMeta } from '@pnpm/resolving.registry.types'

import type { FetchMetadataResult } from '../src/fetch.js'
import { memoizeFetchMetadata } from '../src/memoizeFetchMetadata.js'

const REGISTRY = 'https://registry.npmjs.org/'

function fooFetchResult (): FetchMetadataResult {
  return {
    meta: { name: 'foo' } as PackageMeta,
    jsonText: '{"name":"foo"}',
    etag: '"abc"',
  }
}

test('the initiating caller receives the raw body; cache hits get a body-less clone sharing the same meta', async () => {
  const result = fooFetchResult()
  let calls = 0
  const { fetch } = memoizeFetchMetadata(async () => {
    calls++
    return result
  })

  const first = await fetch('foo', { registry: REGISTRY })
  expect(first).toBe(result)
  if (first.notModified) throw new Error('expected a fresh fetch result')
  expect(first.jsonText).toBe('{"name":"foo"}')

  const second = await fetch('foo', { registry: REGISTRY })
  expect(calls).toBe(1)
  if (second.notModified) throw new Error('expected a cached fetch result')
  expect(second.jsonText).toBeUndefined()
  expect(second.meta).toBe(result.meta)
  expect(second.etag).toBe(result.etag)
  // The clone keeps the original intact for the initiating caller.
  expect(result.jsonText).toBe('{"name":"foo"}')
})

test('callers sharing an in-flight fetch receive the same raw body', async () => {
  let release!: (result: FetchMetadataResult) => void
  const { fetch } = memoizeFetchMetadata(async () => new Promise<FetchMetadataResult>((resolve) => {
    release = resolve
  }))

  const firstPromise = fetch('foo', { registry: REGISTRY })
  const secondPromise = fetch('foo', { registry: REGISTRY })
  release(fooFetchResult())

  const [first, second] = await Promise.all([firstPromise, secondPromise])
  if (first.notModified || second.notModified) throw new Error('expected fresh fetch results')
  expect(first.jsonText).toBe('{"name":"foo"}')
  expect(second).toBe(first)
})

test('requests with different options are cached separately', async () => {
  const fetchedOpts: boolean[] = []
  const { fetch } = memoizeFetchMetadata(async (pkgName, opts) => {
    fetchedOpts.push(opts.fullMetadata === true)
    return fooFetchResult()
  })

  await fetch('foo', { registry: REGISTRY })
  await fetch('foo', { registry: REGISTRY, fullMetadata: true })
  await fetch('foo', { registry: REGISTRY })
  expect(fetchedOpts).toEqual([false, true])
})

test('a rejected fetch is evicted so the next request retries', async () => {
  let calls = 0
  const { fetch } = memoizeFetchMetadata(async () => {
    calls++
    if (calls === 1) throw new Error('network down')
    return fooFetchResult()
  })

  await expect(fetch('foo', { registry: REGISTRY })).rejects.toThrow('network down')
  const retried = await fetch('foo', { registry: REGISTRY })
  if (retried.notModified) throw new Error('expected a fresh fetch result')
  expect(retried.meta.name).toBe('foo')
  expect(calls).toBe(2)
})

test('clear does not let an in-flight fetch repopulate the cache', async () => {
  let calls = 0
  let release!: (result: FetchMetadataResult) => void
  const { fetch, clear } = memoizeFetchMetadata(async () => {
    calls++
    return new Promise<FetchMetadataResult>((resolve) => {
      release = resolve
    })
  })

  const firstPromise = fetch('foo', { registry: REGISTRY })
  clear()
  release(fooFetchResult())
  await firstPromise

  const secondPromise = fetch('foo', { registry: REGISTRY })
  expect(calls).toBe(2)
  release(fooFetchResult())
  await secondPromise
})

test('a rejected fetch does not evict the request that replaced it', async () => {
  let calls = 0
  let release!: (result: FetchMetadataResult) => void
  const { fetch, clear } = memoizeFetchMetadata(async () => {
    calls++
    if (calls === 1) throw new Error('network down')
    return new Promise<FetchMetadataResult>((resolve) => {
      release = resolve
    })
  })

  const firstPromise = fetch('foo', { registry: REGISTRY })
  clear()
  const secondPromise = fetch('foo', { registry: REGISTRY })
  await expect(firstPromise).rejects.toThrow('network down')

  // The eviction must leave the second request's entry in place, so a third
  // caller joins it instead of opening a redundant request.
  const thirdPromise = fetch('foo', { registry: REGISTRY })
  expect(calls).toBe(2)
  release(fooFetchResult())
  const [second, third] = await Promise.all([secondPromise, thirdPromise])
  expect(third).toBe(second)
})

test('clear() empties the cache', async () => {
  let calls = 0
  const { fetch, clear } = memoizeFetchMetadata(async () => {
    calls++
    return fooFetchResult()
  })

  await fetch('foo', { registry: REGISTRY })
  clear()
  await fetch('foo', { registry: REGISTRY })
  expect(calls).toBe(2)
})

test('notModified results pass through unchanged', async () => {
  const { fetch } = memoizeFetchMetadata(async () => ({ notModified: true as const }))

  const first = await fetch('foo', { registry: REGISTRY })
  const second = await fetch('foo', { registry: REGISTRY })
  expect(first.notModified).toBe(true)
  expect(second).toBe(first)
})

test('a throwing condenseSettledMeta falls back to retaining the uncondensed meta', async () => {
  const result = fooFetchResult()
  const { fetch } = memoizeFetchMetadata(async () => result, {
    condenseSettledMeta: () => {
      throw new Error('malformed document')
    },
  })

  const first = await fetch('foo', { registry: REGISTRY })
  expect(first).toBe(result)

  const second = await fetch('foo', { registry: REGISTRY })
  if (second.notModified) throw new Error('expected a cached fetch result')
  expect(second.meta).toBe(result.meta)
  expect(second.jsonText).toBeUndefined()
})

test('condenseSettledMeta narrows the retained meta while the initiating caller sees the original', async () => {
  const result = fooFetchResult()
  const condensed = { name: 'foo' } as PackageMeta
  const { fetch } = memoizeFetchMetadata(async () => result, {
    condenseSettledMeta: (meta) => {
      expect(meta).toBe(result.meta)
      return condensed
    },
  })

  const first = await fetch('foo', { registry: REGISTRY })
  expect(first).toBe(result)

  const second = await fetch('foo', { registry: REGISTRY })
  if (second.notModified) throw new Error('expected a cached fetch result')
  expect(second.meta).toBe(condensed)
  expect(result.meta).not.toBe(condensed)
})
