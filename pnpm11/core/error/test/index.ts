import { expect, test } from '@jest/globals'
import { FetchError, PnpmError, redactUrlCredentials } from '@pnpm/error'

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

test('FetchError strips basic-auth credentials embedded in the request URL', () => {
  const error = new FetchError(
    { url: 'https://user:pass@registry.example/@scope%2fpkg' },
    { status: 403, statusText: 'Forbidden' }
  )
  expect(error.message).toBe('GET https://registry.example/@scope%2fpkg: Forbidden - 403')
})

test('redactUrlCredentials', () => {
  // user:pass@ and user@ userinfo are stripped, regardless of scheme.
  expect(redactUrlCredentials('GET https://user:pass@host/pkg: timed out'))
    .toBe('GET https://host/pkg: timed out')
  expect(redactUrlCredentials('git+ssh://token@host/repo.git'))
    .toBe('git+ssh://host/repo.git')
  // A raw "@" inside the password is stripped up to the last "@" in the
  // authority, so the password tail can't leak.
  expect(redactUrlCredentials('GET https://user:p@ss@host/pkg: 403'))
    .toBe('GET https://host/pkg: 403')
  // An "@" in the path/query (after the authority) is preserved.
  expect(redactUrlCredentials('https://host/path?to=a@b'))
    .toBe('https://host/path?to=a@b')
  // A credential-free URL is left untouched.
  expect(redactUrlCredentials('GET https://host/pkg: timed out'))
    .toBe('GET https://host/pkg: timed out')
  // A bare "://" with no preceding scheme character is not a URL authority, so
  // a later "@" is preserved.
  expect(redactUrlCredentials('a :// b@c')).toBe('a :// b@c')
  // Multiple credentialed URLs in one message are all redacted.
  expect(redactUrlCredentials('a https://u:p@h1/x and b https://t@h2/y'))
    .toBe('a https://h1/x and b https://h2/y')
})
