import { describe, expect, test } from '@jest/globals'

import { readResponseBodyCapped } from '../../src/tarball/readResponseBodyCapped.js'

describe('readResponseBodyCapped', () => {
  test('returns an empty buffer for a response without a body', async () => {
    const body = await readResponseBodyCapped(new Response(null), 4)
    expect(body).toEqual(Buffer.alloc(0))
  })

  test('returns a response within the limit', async () => {
    const body = await readResponseBodyCapped(new Response(new ReadableStream({
      start: (controller) => {
        controller.enqueue(new TextEncoder().encode('ab'))
        controller.enqueue(new TextEncoder().encode('cd'))
        controller.close()
      },
    })), 4)
    expect(body?.toString()).toBe('abcd')
  })

  test('stops reading a response that exceeds the limit', async () => {
    let cancelled = false
    const response = new Response(new ReadableStream({
      cancel: () => {
        cancelled = true
      },
      start: (controller) => {
        controller.enqueue(new Uint8Array([1, 2, 3]))
        controller.enqueue(new Uint8Array([4, 5, 6]))
      },
    }))

    await expect(readResponseBodyCapped(response, 4)).resolves.toBeUndefined()
    expect(cancelled).toBe(true)
  })
})
