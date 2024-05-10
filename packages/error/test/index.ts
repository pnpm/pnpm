import { FetchError } from '@pnpm/error'

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
