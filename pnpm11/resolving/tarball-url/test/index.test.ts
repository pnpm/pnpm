import { describe, expect, test } from '@jest/globals'

import { getNpmTarballUrl, isCanonicalRegistryTarballUrl } from '../src/index.js'

describe('getNpmTarballUrl', () => {
  test('create simple URL', () => {
    expect(getNpmTarballUrl('foo', '1.0.0')).toBe('https://registry.npmjs.org/foo/-/foo-1.0.0.tgz')
  })

  test('create URL of scoped package', () => {
    expect(getNpmTarballUrl('@types/semver', '5.3.31')).toBe('https://registry.npmjs.org/@types/semver/-/semver-5.3.31.tgz')
  })

  test('create URL with custom registry', () => {
    expect(getNpmTarballUrl('foo', '1.0.0', { registry: 'http://sinopia' })).toBe('http://sinopia/foo/-/foo-1.0.0.tgz')
  })

  test('create URL with custom registry that has a trailing slash', () => {
    expect(getNpmTarballUrl('foo', '1.0.0', { registry: 'http://sinopia/' })).toBe('http://sinopia/foo/-/foo-1.0.0.tgz')
  })

  test('ignore the build metadata in the version', () => {
    expect(getNpmTarballUrl('foo', '1.0.0+abc')).toBe('https://registry.npmjs.org/foo/-/foo-1.0.0.tgz')
  })

  test('create URL with a custom registry that includes a path', () => {
    expect(getNpmTarballUrl('foo', '1.0.0', { registry: 'https://npm.pkg.github.com/owner' })).toBe('https://npm.pkg.github.com/owner/foo/-/foo-1.0.0.tgz')
  })
})

describe('isCanonicalRegistryTarballUrl', () => {
  const registry = 'https://registry.npmjs.org/'

  test('is true for the URL derived from name, version, and registry', () => {
    const tarball = getNpmTarballUrl('lodash', '4.17.21', { registry })
    expect(isCanonicalRegistryTarballUrl(tarball, { name: 'lodash', version: '4.17.21' }, { registry })).toBe(true)
  })

  test('is true for a scoped package using unencoded slash', () => {
    const tarball = getNpmTarballUrl('@babel/core', '7.0.0', { registry })
    expect(isCanonicalRegistryTarballUrl(tarball, { name: '@babel/core', version: '7.0.0' }, { registry })).toBe(true)
  })

  test.each([
    'https://registry.npmjs.org/@babel%2fcore/-/core-7.0.0.tgz',
    'https://registry.npmjs.org/@babel%2Fcore/-/core-7.0.0.tgz',
  ])('is true on the public registry, which also serves the encoded path: %s', (tarball) => {
    expect(isCanonicalRegistryTarballUrl(tarball, { name: '@babel/core', version: '7.0.0' }, { registry })).toBe(true)
  })

  test.each([
    'https://npm.example.com/@babel%2fcore/-/core-7.0.0.tgz',
    'https://npm.example.com/@babel%2Fcore/-/core-7.0.0.tgz',
  ])('is false on any other registry, which may serve only the encoded path: %s', (tarball) => {
    expect(isCanonicalRegistryTarballUrl(tarball, { name: '@babel/core', version: '7.0.0' }, { registry: 'https://npm.example.com/' })).toBe(false)
  })

  test('ignores the protocol', () => {
    const tarball = getNpmTarballUrl('lodash', '4.17.21', { registry }).replace('https://', 'http://')
    expect(isCanonicalRegistryTarballUrl(tarball, { name: 'lodash', version: '4.17.21' }, { registry })).toBe(true)
  })

  test('is false for a proxy URL on a non-canonical path', () => {
    const tarball = 'http://localhost:54321/tarballs/npm/lodash/4.17.21/abc'
    expect(isCanonicalRegistryTarballUrl(tarball, { name: 'lodash', version: '4.17.21' }, { registry })).toBe(false)
  })

  test('is false when a second :// follows the canonical URL', () => {
    const tarball = `${getNpmTarballUrl('lodash', '4.17.21', { registry })}://suffix`
    expect(isCanonicalRegistryTarballUrl(tarball, { name: 'lodash', version: '4.17.21' }, { registry })).toBe(false)
  })

  test('is false when the version differs', () => {
    const tarball = getNpmTarballUrl('lodash', '4.17.20', { registry })
    expect(isCanonicalRegistryTarballUrl(tarball, { name: 'lodash', version: '4.17.21' }, { registry })).toBe(false)
  })
})

describe('an Artifactory registry, which keeps the scope in the tarball filename', () => {
  const registry = 'https://artifactory.example/artifactory/api/npm/npm-virtual/'
  const serverType = 'artifactory'

  test('builds the scoped filename for a scoped package', () => {
    expect(getNpmTarballUrl('@acme/widget', '1.2.3', { registry, serverType }))
      .toBe(`${registry}@acme/widget/-/@acme/widget-1.2.3.tgz`)
  })

  test('builds the same URL as the npm layout for an unscoped package', () => {
    expect(getNpmTarballUrl('widget', '1.2.3', { registry, serverType }))
      .toBe(getNpmTarballUrl('widget', '1.2.3', { registry }))
  })

  test('drops the build metadata from the version, like the npm layout', () => {
    expect(getNpmTarballUrl('@acme/widget', '1.2.3+build.4', { registry, serverType }))
      .toBe(`${registry}@acme/widget/-/@acme/widget-1.2.3.tgz`)
  })

  test.each([
    ['1.2.3'],
    ['1.2.3-beta.1'],
  ])('recognizes its own URL for version %s as reconstructible', (version) => {
    const tarball = getNpmTarballUrl('@acme/widget', version, { registry, serverType })
    expect(isCanonicalRegistryTarballUrl(tarball, { name: '@acme/widget', version }, { registry, serverType })).toBe(true)
  })

  test('does not recognize the npm-layout URL, which it does not serve', () => {
    const tarball = getNpmTarballUrl('@acme/widget', '1.2.3', { registry })
    expect(isCanonicalRegistryTarballUrl(tarball, { name: '@acme/widget', version: '1.2.3' }, { registry, serverType })).toBe(false)
  })

  test('is not recognized by a registry left on the npm layout', () => {
    const tarball = getNpmTarballUrl('@acme/widget', '1.2.3', { registry, serverType })
    expect(isCanonicalRegistryTarballUrl(tarball, { name: '@acme/widget', version: '1.2.3' }, { registry })).toBe(false)
  })

  test('does not treat the encoded path as reconstructible', () => {
    const encodedName = '@acme/widget'.split('/').join('%2f')
    const tarball = `${registry}${encodedName}/-/${encodedName}-1.2.3.tgz`
    expect(isCanonicalRegistryTarballUrl(tarball, { name: '@acme/widget', version: '1.2.3' }, { registry, serverType })).toBe(false)
  })
})

describe('a registry declared to behave like the npm registry', () => {
  const registry = 'https://npm.example.com/'
  const tarball = 'https://npm.example.com/@babel%2Fcore/-/core-7.0.0.tgz'
  const pkg = { name: '@babel/core', version: '7.0.0' }

  test('is true for the encoded scoped path, which it serves like npmjs does', () => {
    expect(isCanonicalRegistryTarballUrl(tarball, pkg, { registry, serverType: 'npm' })).toBe(true)
  })

  test('is false for the same URL while undeclared, which may serve only the encoded path', () => {
    expect(isCanonicalRegistryTarballUrl(tarball, pkg, { registry })).toBe(false)
  })

  test('builds the same URL as an undeclared registry', () => {
    expect(getNpmTarballUrl('@babel/core', '7.0.0', { registry, serverType: 'npm' }))
      .toBe(getNpmTarballUrl('@babel/core', '7.0.0', { registry }))
  })
})

test('the public npm registry keeps its encoded-path leniency without being declared', () => {
  const registry = 'https://registry.npmjs.org/'
  const tarball = 'https://registry.npmjs.org/@babel%2Fcore/-/core-7.0.0.tgz'
  expect(isCanonicalRegistryTarballUrl(tarball, { name: '@babel/core', version: '7.0.0' }, { registry })).toBe(true)
})
