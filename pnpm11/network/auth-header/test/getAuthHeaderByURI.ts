import { expect, test } from '@jest/globals'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'

const configByUri = {
  '//reg.com/': { '@': { authToken: 'abc123' } },
  '//reg.co/tarballs/': { '@': { authToken: 'xxx' } },
  '//reg.gg:8888/': { '@': { authToken: '0000' } },
  '//custom.domain.com/artifactory/api/npm/npm-virtual/': { '@': { authToken: 'xyz' } },
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
    '//reg.com/': { '@': { authToken: 'abc123' } },
  })
  expect(getAuthHeaderByURI('https://reg.com:443/')).toBe('Bearer abc123')
  expect(getAuthHeaderByURI('http://reg.com:80/')).toBe('Bearer abc123')
})

test('returns undefined when the auth header is not found', () => {
  expect(createGetAuthHeaderByURI({})('http://reg.com')).toBeUndefined()
})

test('getAuthHeaderByURI() when the registry has pathnames', () => {
  const getAuthHeaderByURI = createGetAuthHeaderByURI({
    '//npm.pkg.github.com/pnpm/': { '@': { authToken: 'abc123' } },
  })
  expect(getAuthHeaderByURI('https://npm.pkg.github.com/pnpm')).toBe('Bearer abc123')
  expect(getAuthHeaderByURI('https://npm.pkg.github.com/pnpm/')).toBe('Bearer abc123')
  expect(getAuthHeaderByURI('https://npm.pkg.github.com/pnpm/foo')).toBe('Bearer abc123')
  expect(getAuthHeaderByURI('https://npm.pkg.github.com/pnpm/foo/')).toBe('Bearer abc123')
  expect(getAuthHeaderByURI('https://npm.pkg.github.com/pnpm/foo/-/foo-1.0.0.tgz')).toBe('Bearer abc123')
})

test('getAuthHeaderByURI() with basic auth via basicAuth', () => {
  const getAuthHeaderByURI = createGetAuthHeaderByURI({
    '//reg.com/': { '@': { basicAuth: { username: 'user', password: 'pass' } } },
  })
  expect(getAuthHeaderByURI('https://reg.com/')).toBe('Basic ' + btoa('user:pass'))
})

test('getAuthHeaderByURI() prefers package scope auth over registry auth', () => {
  const getAuthHeaderByURI = createGetAuthHeaderByURI({
    '//npm.pkg.github.com/': {
      '@': { authToken: 'registry-token' },
      '@orgA': { authToken: 'org-a-token' },
      '@orgB': { authToken: 'org-b-token' },
    },
  })
  expect(getAuthHeaderByURI('https://npm.pkg.github.com/', { pkgName: '@orgA/pkg' })).toBe('Bearer org-a-token')
  expect(getAuthHeaderByURI('https://npm.pkg.github.com/', { pkgName: '@orgB/pkg' })).toBe('Bearer org-b-token')
  expect(getAuthHeaderByURI('https://npm.pkg.github.com/', { pkgName: '@orgC/pkg' })).toBe('Bearer registry-token')
  expect(getAuthHeaderByURI('https://npm.pkg.github.com/', { pkgName: 'pkg' })).toBe('Bearer registry-token')
  expect(getAuthHeaderByURI('https://npm.pkg.github.com/download/pkg.tgz', { pkgName: '@orgA/pkg' })).toBe('Bearer org-a-token')
})

test('getAuthHeaderByURI() keeps registry path when matching package scope auth', () => {
  const getAuthHeaderByURI = createGetAuthHeaderByURI({
    '//reg.com/npm/': {
      '@': { authToken: 'registry-token' },
      '@orgA': { authToken: 'org-a-token' },
    },
  })
  expect(getAuthHeaderByURI('https://reg.com/npm/', { pkgName: '@orgA/pkg' })).toBe('Bearer org-a-token')
  expect(getAuthHeaderByURI('https://reg.com/npm/pkg/-/pkg-1.0.0.tgz', { pkgName: '@orgA/pkg' })).toBe('Bearer org-a-token')
  expect(getAuthHeaderByURI('https://reg.com/npm/', { pkgName: '@orgB/pkg' })).toBe('Bearer registry-token')
})

test('getAuthHeaderByURI() basic auth in URL overrides package scope auth', () => {
  const getAuthHeaderByURI = createGetAuthHeaderByURI({
    '//reg.com/': {
      '@orgA': { authToken: 'org-a-token' },
    },
  })
  expect(getAuthHeaderByURI('https://user:secret@reg.com/', { pkgName: '@orgA/pkg' })).toBe('Basic ' + btoa('user:secret'))
})
