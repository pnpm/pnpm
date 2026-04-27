import { expect, test } from '@jest/globals'
import { FetchError, PnpmError } from '@pnpm/error'

test('PnpmError exposes cause when provided', () => {
  const cause = new Error('original failure')
  const error = new PnpmError('TEST_CODE', 'something went wrong', { cause })
  expect(error.cause).toBe(cause)
  expect(error.message).toBe('something went wrong')
  expect(error.code).toBe('ERR_PNPM_TEST_CODE')
})

test('PnpmError cause is undefined when omitted', () => {
  const error = new PnpmError('TEST_CODE', 'something went wrong')
  expect(error.cause).toBeUndefined()
})

test('PnpmError cause works with non-Error values', () => {
  const error = new PnpmError('TEST_CODE', 'something went wrong', { cause: 'string cause' })
  expect(error.cause).toBe('string cause')
})

test('FetchError escapes auth tokens', () => {
  const error = new FetchError(
    { url: 'https://foo.com', authHeaderValue: 'Bearer 00000000000000000000' },
    { status: 401, statusText: 'Unauthorized' }
  )
  expect(error.message).toBe('GET https://foo.com: Unauthorized - 401')
  expect(error.hint).toBe('An authorization header was used: Bearer 0000[hidden]')
  expect(error.request.authHeaderValue).toBe('Bearer 0000[hidden]')
})

test('FetchError escapes short auth tokens', () => {
  const error = new FetchError(
    { url: 'https://foo.com', authHeaderValue: 'Bearer 0000000000' },
    { status: 401, statusText: 'Unauthorized' }
  )
  expect(error.message).toBe('GET https://foo.com: Unauthorized - 401')
  expect(error.hint).toBe('An authorization header was used: Bearer [hidden]')
  expect(error.request.authHeaderValue).toBe('Bearer [hidden]')
})

test('FetchError escapes non-standard auth header', () => {
  const error = new FetchError(
    { url: 'https://foo.com', authHeaderValue: '0000000000' },
    { status: 401, statusText: 'Unauthorized' }
  )
  expect(error.message).toBe('GET https://foo.com: Unauthorized - 401')
  expect(error.hint).toBe('An authorization header was used: [hidden]')
  expect(error.request.authHeaderValue).toBe('[hidden]')
})
