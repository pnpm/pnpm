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
    expect(isCanonicalRegistryTarballUrl(tarball, { name: 'lodash', version: '4.17.21' }, registry)).toBe(true)
  })

  test('is true for a scoped package, matching the npm %2f escaping', () => {
    const tarball = getNpmTarballUrl('@babel/core', '7.0.0', { registry })
    expect(isCanonicalRegistryTarballUrl(tarball, { name: '@babel/core', version: '7.0.0' }, registry)).toBe(true)
  })

  test('is true for a scoped package using uppercase %2F escaping', () => {
    const tarball = 'https://registry.npmjs.org/@babel%2Fcore/-/core-7.0.0.tgz'
    expect(isCanonicalRegistryTarballUrl(tarball, { name: '@babel/core', version: '7.0.0' }, registry)).toBe(true)
  })

  test('ignores the protocol', () => {
    const tarball = getNpmTarballUrl('lodash', '4.17.21', { registry }).replace('https://', 'http://')
    expect(isCanonicalRegistryTarballUrl(tarball, { name: 'lodash', version: '4.17.21' }, registry)).toBe(true)
  })

  test('is false for a proxy URL on a non-canonical path', () => {
    const tarball = 'http://localhost:54321/tarballs/npm/lodash/4.17.21/abc'
    expect(isCanonicalRegistryTarballUrl(tarball, { name: 'lodash', version: '4.17.21' }, registry)).toBe(false)
  })

  test('is false when a second :// follows the canonical URL', () => {
    const tarball = `${getNpmTarballUrl('lodash', '4.17.21', { registry })}://suffix`
    expect(isCanonicalRegistryTarballUrl(tarball, { name: 'lodash', version: '4.17.21' }, registry)).toBe(false)
  })

  test('is false when the version differs', () => {
    const tarball = getNpmTarballUrl('lodash', '4.17.20', { registry })
    expect(isCanonicalRegistryTarballUrl(tarball, { name: 'lodash', version: '4.17.21' }, registry)).toBe(false)
  })
})
