import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'

const opts = {
  allSettings: {
    '//reg.com/:_authToken': 'abc123',
    '//reg.co/tarballs/:_authToken': 'xxx',
    '//reg.gg:8888/:_authToken': '0000',
    '//custom.domain.com/artifactory/api/npm/npm-virtual/:_authToken': 'xyz',
  },
  userSettings: {},
}

test('getAuthHeaderByURI()', () => {
  const getAuthHeaderByURI = createGetAuthHeaderByURI(opts)
  expect(getAuthHeaderByURI('https://reg.com/')).toBe('Bearer abc123')
  expect(getAuthHeaderByURI('https://reg.com/foo/-/foo-1.0.0.tgz')).toBe('Bearer abc123')
  expect(getAuthHeaderByURI('https://reg.com:8080/foo/-/foo-1.0.0.tgz')).toBe('Bearer abc123')
  expect(getAuthHeaderByURI('https://reg.io/foo/-/foo-1.0.0.tgz')).toBe(undefined)
  expect(getAuthHeaderByURI('https://reg.co/tarballs/foo/-/foo-1.0.0.tgz')).toBe('Bearer xxx')
  expect(getAuthHeaderByURI('https://reg.gg:8888/foo/-/foo-1.0.0.tgz')).toBe('Bearer 0000')
  expect(getAuthHeaderByURI('https://reg.gg:8888/foo/-/foo-1.0.0.tgz')).toBe('Bearer 0000')
})

test('getAuthHeaderByURI() basic auth without settings', () => {
  const getAuthHeaderByURI = createGetAuthHeaderByURI({
    allSettings: {},
  })
  expect(getAuthHeaderByURI('https://user:secret@reg.io/')).toBe('Basic ' + btoa('user:secret'))
  expect(getAuthHeaderByURI('https://user:@reg.io/')).toBe('Basic ' + btoa('user:'))
  expect(getAuthHeaderByURI('https://:secret@reg.io/')).toBe('Basic ' + btoa(':secret'))
  expect(getAuthHeaderByURI('https://user@reg.io/')).toBe('Basic ' + btoa('user:'))
})

test('getAuthHeaderByURI() basic auth with settings', () => {
  const getAuthHeaderByURI = createGetAuthHeaderByURI(opts)
  expect(getAuthHeaderByURI('https://user:secret@reg.com/')).toBe('Basic ' + btoa('user:secret'))
  expect(getAuthHeaderByURI('https://user:secret@reg.com/foo/-/foo-1.0.0.tgz')).toBe('Basic ' + btoa('user:secret'))
  expect(getAuthHeaderByURI('https://user:secret@reg.com:8080/foo/-/foo-1.0.0.tgz')).toBe('Basic ' + btoa('user:secret'))
  expect(getAuthHeaderByURI('https://user:secret@reg.io/foo/-/foo-1.0.0.tgz')).toBe('Basic ' + btoa('user:secret'))
  expect(getAuthHeaderByURI('https://user:secret@reg.co/tarballs/foo/-/foo-1.0.0.tgz')).toBe('Basic ' + btoa('user:secret'))
  expect(getAuthHeaderByURI('https://user:secret@reg.gg:8888/foo/-/foo-1.0.0.tgz')).toBe('Basic ' + btoa('user:secret'))
  expect(getAuthHeaderByURI('https://user:secret@reg.gg:8888/foo/-/foo-1.0.0.tgz')).toBe('Basic ' + btoa('user:secret'))
})

test('getAuthHeaderByURI() https port 443 checks', () => {
  const getAuthHeaderByURI = createGetAuthHeaderByURI(opts)
  expect(getAuthHeaderByURI('https://custom.domain.com:443/artifactory/api/npm/npm-virtual/')).toBe('Bearer xyz')
  expect(getAuthHeaderByURI('https://custom.domain.com:443/artifactory/api/npm/')).toBe(undefined)
  expect(getAuthHeaderByURI('https://custom.domain.com:443/artifactory/api/npm/-/@platform/device-utils-1.0.0.tgz')).toBe(undefined)
  expect(getAuthHeaderByURI('https://custom.domain.com:443/artifactory/api/npm/npm-virtual/@platform/device-utils/-/@platform/device-utils-1.0.0.tgz')).toBe('Bearer xyz')
})

test('getAuthHeaderByURI() when default ports are specified', () => {
  const getAuthHeaderByURI = createGetAuthHeaderByURI({
    allSettings: {
      '//reg.com/:_authToken': 'abc123',
    },
    userSettings: {},
  })
  expect(getAuthHeaderByURI('https://reg.com:443/')).toBe('Bearer abc123')
  expect(getAuthHeaderByURI('http://reg.com:80/')).toBe('Bearer abc123')
})

test('returns undefined when the auth header is not found', () => {
  expect(createGetAuthHeaderByURI({ allSettings: {}, userSettings: {} })('http://reg.com')).toBe(undefined)
})

test('getAuthHeaderByURI() when the registry has pathnames', () => {
  const getAuthHeaderByURI = createGetAuthHeaderByURI({
    allSettings: {
      '//npm.pkg.github.com/pnpm/:_authToken': 'abc123',
    },
    userSettings: {},
  })
  expect(getAuthHeaderByURI('https://npm.pkg.github.com/pnpm')).toBe('Bearer abc123')
  expect(getAuthHeaderByURI('https://npm.pkg.github.com/pnpm/')).toBe('Bearer abc123')
  expect(getAuthHeaderByURI('https://npm.pkg.github.com/pnpm/foo')).toBe('Bearer abc123')
  expect(getAuthHeaderByURI('https://npm.pkg.github.com/pnpm/foo/')).toBe('Bearer abc123')
  expect(getAuthHeaderByURI('https://npm.pkg.github.com/pnpm/foo/-/foo-1.0.0.tgz')).toBe('Bearer abc123')
  expect(getAuthHeaderByURI('https://npm.pkg.github.com/pnpm/foo/-/foo-1.0.0.tgz')).toBe('Bearer abc123')
  expect(getAuthHeaderByURI('https://npm.pkg.github.com/pnpm/foo/-/foo-1.0.0.tgz')).toBe('Bearer abc123')
  expect(getAuthHeaderByURI('https://npm.pkg.github.com/pnpm/foo/-/foo-1.0.0.tgz')).toBe('Bearer abc123')
  expect(getAuthHeaderByURI('https://npm.pkg.github.com/pnpm/foo/-/foo-1.0.0.tgz')).toBe('Bearer abc123')
})
