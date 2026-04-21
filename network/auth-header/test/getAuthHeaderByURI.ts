import { expect, test } from '@jest/globals'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'

const configByUri = {
  '//reg.com/': { creds: { authToken: 'abc123' } },
  '//reg.co/tarballs/': { creds: { authToken: 'xxx' } },
  '//reg.gg:8888/': { creds: { authToken: '0000' } },
  '//custom.domain.com/artifactory/api/npm/npm-virtual/': { creds: { authToken: 'xyz' } },
}

test('getAuthHeaderByURI()', () => {
  const getAuthHeaderByURI = createGetAuthHeaderByURI(configByUri)
  expect(getAuthHeaderByURI('https://reg.com/')).toBe('Bearer abc123')
  expect(getAuthHeaderByURI('https://reg.com/foo/-/foo-1.0.0.tgz')).toBe('Bearer abc123')
  expect(getAuthHeaderByURI('https://reg.com:8080/foo/-/foo-1.0.0.tgz')).toBe('Bearer abc123')
  expect(getAuthHeaderByURI('https://reg.io/foo/-/foo-1.0.0.tgz')).toBeUndefined()
  expect(getAuthHeaderByURI('https://reg.co/tarballs/foo/-/foo-1.0.0.tgz')).toBe('Bearer xxx')
  expect(getAuthHeaderByURI('https://reg.gg:8888/foo/-/foo-1.0.0.tgz')).toBe('Bearer 0000')
  expect(getAuthHeaderByURI('https://reg.gg:8888/foo/-/foo-1.0.0.tgz')).toBe('Bearer 0000')
})

test('getAuthHeaderByURI() basic auth without settings', () => {
  const getAuthHeaderByURI = createGetAuthHeaderByURI({})
  expect(getAuthHeaderByURI('https://user:secret@reg.io/')).toBe('Basic ' + btoa('user:secret'))
  expect(getAuthHeaderByURI('https://user:@reg.io/')).toBe('Basic ' + btoa('user:'))
  expect(getAuthHeaderByURI('https://:secret@reg.io/')).toBe('Basic ' + btoa(':secret'))
  expect(getAuthHeaderByURI('https://user@reg.io/')).toBe('Basic ' + btoa('user:'))
})

test('getAuthHeaderByURI() basic auth with settings', () => {
  const getAuthHeaderByURI = createGetAuthHeaderByURI(configByUri)
  expect(getAuthHeaderByURI('https://user:secret@reg.com/')).toBe('Basic ' + btoa('user:secret'))
  expect(getAuthHeaderByURI('https://user:secret@reg.com/foo/-/foo-1.0.0.tgz')).toBe('Basic ' + btoa('user:secret'))
  expect(getAuthHeaderByURI('https://user:secret@reg.com:8080/foo/-/foo-1.0.0.tgz')).toBe('Basic ' + btoa('user:secret'))
  expect(getAuthHeaderByURI('https://user:secret@reg.io/foo/-/foo-1.0.0.tgz')).toBe('Basic ' + btoa('user:secret'))
  expect(getAuthHeaderByURI('https://user:secret@reg.co/tarballs/foo/-/foo-1.0.0.tgz')).toBe('Basic ' + btoa('user:secret'))
  expect(getAuthHeaderByURI('https://user:secret@reg.gg:8888/foo/-/foo-1.0.0.tgz')).toBe('Basic ' + btoa('user:secret'))
  expect(getAuthHeaderByURI('https://user:secret@reg.gg:8888/foo/-/foo-1.0.0.tgz')).toBe('Basic ' + btoa('user:secret'))
})

test('getAuthHeaderByURI() https port 443 checks', () => {
  const getAuthHeaderByURI = createGetAuthHeaderByURI(configByUri)
  expect(getAuthHeaderByURI('https://custom.domain.com:443/artifactory/api/npm/npm-virtual/')).toBe('Bearer xyz')
  expect(getAuthHeaderByURI('https://custom.domain.com:443/artifactory/api/npm/')).toBeUndefined()
  expect(getAuthHeaderByURI('https://custom.domain.com:443/artifactory/api/npm/-/@platform/device-utils-1.0.0.tgz')).toBeUndefined()
  expect(getAuthHeaderByURI('https://custom.domain.com:443/artifactory/api/npm/npm-virtual/@platform/device-utils/-/@platform/device-utils-1.0.0.tgz')).toBe('Bearer xyz')
})

test('getAuthHeaderByURI() when default ports are specified', () => {
  const getAuthHeaderByURI = createGetAuthHeaderByURI({
    '//reg.com/': { creds: { authToken: 'abc123' } },
  })
  expect(getAuthHeaderByURI('https://reg.com:443/')).toBe('Bearer abc123')
  expect(getAuthHeaderByURI('http://reg.com:80/')).toBe('Bearer abc123')
})

test('returns undefined when the auth header is not found', () => {
  expect(createGetAuthHeaderByURI({})('http://reg.com')).toBeUndefined()
})

test('getAuthHeaderByURI() when the registry has pathnames', () => {
  const getAuthHeaderByURI = createGetAuthHeaderByURI({
    '//npm.pkg.github.com/pnpm/': { creds: { authToken: 'abc123' } },
  })
  expect(getAuthHeaderByURI('https://npm.pkg.github.com/pnpm')).toBe('Bearer abc123')
  expect(getAuthHeaderByURI('https://npm.pkg.github.com/pnpm/')).toBe('Bearer abc123')
  expect(getAuthHeaderByURI('https://npm.pkg.github.com/pnpm/foo')).toBe('Bearer abc123')
  expect(getAuthHeaderByURI('https://npm.pkg.github.com/pnpm/foo/')).toBe('Bearer abc123')
  expect(getAuthHeaderByURI('https://npm.pkg.github.com/pnpm/foo/-/foo-1.0.0.tgz')).toBe('Bearer abc123')
})

test('getAuthHeaderByURI() with default registry auth', () => {
  const getAuthHeaderByURI = createGetAuthHeaderByURI(
    { '': { creds: { authToken: 'default-token' } } },
    'https://registry.npmjs.org/'
  )
  expect(getAuthHeaderByURI('https://registry.npmjs.org/')).toBe('Bearer default-token')
  expect(getAuthHeaderByURI('https://registry.npmjs.org/foo/-/foo-1.0.0.tgz')).toBe('Bearer default-token')
})

test('getAuthHeaderByURI() with basic auth via basicAuth', () => {
  const getAuthHeaderByURI = createGetAuthHeaderByURI({
    '//reg.com/': { creds: { basicAuth: { username: 'user', password: 'pass' } } },
  })
  expect(getAuthHeaderByURI('https://reg.com/')).toBe('Basic ' + btoa('user:pass'))
})
