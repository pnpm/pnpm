/// <reference path="../../../__typings__/index.d.ts"/>
import { readBodyWithLimit, readJsonWithLimit, ResponseBodyTooLargeError, createFetchFromRegistry } from '@pnpm/fetch'
import nock from 'nock'

afterEach(() => {
  nock.cleanAll()
})

describe('readBodyWithLimit', () => {
  test('reads body when size is within limit', async () => {
    const body = JSON.stringify({ name: 'test-package' })
    nock('http://registry.test/')
      .get('/package')
      .reply(200, body, { 'content-length': body.length.toString() })

    const fetchFromRegistry = createFetchFromRegistry({})
    const response = await fetchFromRegistry('http://registry.test/package')

    const buffer = await readBodyWithLimit(response, 1024 * 1024, 'http://registry.test/package')
    expect(buffer.toString()).toBe(body)
  })

  test('throws ResponseBodyTooLargeError when Content-Length exceeds limit', async () => {
    // Set Content-Length to 10MB but only allow 1KB
    nock('http://registry.test/')
      .get('/large-package')
      .reply(200, 'small', { 'content-length': (10 * 1024 * 1024).toString() })

    const fetchFromRegistry = createFetchFromRegistry({})
    const response = await fetchFromRegistry('http://registry.test/large-package')

    await expect(
      readBodyWithLimit(response, 1024, 'http://registry.test/large-package')
    ).rejects.toThrow(ResponseBodyTooLargeError)

    await expect(
      readBodyWithLimit(response, 1024, 'http://registry.test/large-package')
    ).rejects.toMatchObject({
      code: 'ERR_PNPM_RESPONSE_BODY_TOO_LARGE',
      maxSize: 1024,
      receivedSize: 10 * 1024 * 1024,
    })
  })

  test('throws ResponseBodyTooLargeError when streaming body exceeds limit (no Content-Length)', async () => {
    // Create a body larger than the limit, but don't send Content-Length header
    const largeBody = 'A'.repeat(2048) // 2KB
    nock('http://registry.test/')
      .get('/streaming-large')
      .reply(200, largeBody) // No Content-Length header

    const fetchFromRegistry = createFetchFromRegistry({})
    const response = await fetchFromRegistry('http://registry.test/streaming-large')

    await expect(
      readBodyWithLimit(response, 1024, 'http://registry.test/streaming-large') // 1KB limit
    ).rejects.toThrow(ResponseBodyTooLargeError)
  })
})

describe('readJsonWithLimit', () => {
  test('parses JSON when size is within limit', async () => {
    const body = { name: 'test-package', version: '1.0.0' }
    const bodyStr = JSON.stringify(body)
    nock('http://registry.test/')
      .get('/json-package')
      .reply(200, bodyStr, { 'content-length': bodyStr.length.toString() })

    const fetchFromRegistry = createFetchFromRegistry({})
    const response = await fetchFromRegistry('http://registry.test/json-package')

    const result = await readJsonWithLimit<typeof body>(response, 1024 * 1024, 'http://registry.test/json-package')
    expect(result).toEqual(body)
  })

  test('throws ResponseBodyTooLargeError for large JSON (Content-Length check)', async () => {
    nock('http://registry.test/')
      .get('/large-json')
      .reply(200, '{}', { 'content-length': (100 * 1024 * 1024).toString() })

    const fetchFromRegistry = createFetchFromRegistry({})
    const response = await fetchFromRegistry('http://registry.test/large-json')

    await expect(
      readJsonWithLimit(response, 1024, 'http://registry.test/large-json')
    ).rejects.toThrow(ResponseBodyTooLargeError)
  })

  test('throws SyntaxError for invalid JSON', async () => {
    const invalidJson = 'not valid json'
    nock('http://registry.test/')
      .get('/invalid-json')
      .reply(200, invalidJson, { 'content-length': invalidJson.length.toString() })

    const fetchFromRegistry = createFetchFromRegistry({})
    const response = await fetchFromRegistry('http://registry.test/invalid-json')

    await expect(
      readJsonWithLimit(response, 1024 * 1024, 'http://registry.test/invalid-json')
    ).rejects.toThrow(SyntaxError)
  })
})
