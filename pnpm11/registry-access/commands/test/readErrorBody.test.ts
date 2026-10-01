import { describe, expect, it } from '@jest/globals'

import { readErrorBody } from '../src/common.js'

const ERROR_BODY_LIMIT = 64 * 1024

function responseFromChunks (chunks: Uint8Array[]): Response {
  const body = new ReadableStream<Uint8Array>({
    start (controller) {
      for (const chunk of chunks) controller.enqueue(chunk)
      controller.close()
    },
  })
  return new Response(body)
}

describe('readErrorBody', () => {
  it('returns a body at the limit unchanged', async () => {
    const body = await readErrorBody(responseFromChunks([new Uint8Array(ERROR_BODY_LIMIT).fill(0x61)]))
    expect(body).toBe('a'.repeat(ERROR_BODY_LIMIT))
  })

  it('marks a body as truncated when more data follows a chunk that ends exactly at the limit', async () => {
    const body = await readErrorBody(responseFromChunks([
      new Uint8Array(ERROR_BODY_LIMIT).fill(0x61),
      new Uint8Array(10).fill(0x62),
    ]))
    expect(body).toBe(`${'a'.repeat(ERROR_BODY_LIMIT)} (response body truncated)`)
  })

  it('marks a body as truncated when a chunk crosses the limit', async () => {
    const body = await readErrorBody(responseFromChunks([new Uint8Array(ERROR_BODY_LIMIT + 5).fill(0x61)]))
    expect(body).toBe(`${'a'.repeat(ERROR_BODY_LIMIT)} (response body truncated)`)
  })
})
