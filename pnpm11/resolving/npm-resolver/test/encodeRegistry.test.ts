import path from 'node:path'
import util from 'node:util'

import { describe, expect, test } from '@jest/globals'
import { ABBREVIATED_META_DIR } from '@pnpm/constants'

import { decodeRegistry, encodeRegistry } from '../src/encodeRegistry.js'
import { getPkgMirrorPath } from '../src/pickPackage.js'

// The `%2F` separator runs into the path segment after it, which the spell
// checker reads as one word.
// cspell:ignore Fartifactory, Fregistry, Fteam

describe('encodeRegistry', () => {
  test('host only', () => {
    expect(encodeRegistry('https://registry.npmjs.org/')).toBe('https%3A+registry.npmjs.org')
    expect(encodeRegistry('https://registry.npmjs.org')).toBe('https%3A+registry.npmjs.org')
    expect(encodeRegistry('https://npm.example:8443/')).toBe('https%3A+npm.example+8443')
    expect(encodeRegistry('https://npm.example:443/')).toBe('https%3A+npm.example')
    expect(encodeRegistry('http://[::1]:8080/')).toBe('http%3A+%5B%3A%3A1%5D+8080')
  })
  test('keeps the path', () => {
    expect(encodeRegistry('https://releases.jfrog.io/artifactory/api/npm/team-a/'))
      .toBe('https%3A+releases.jfrog.io%2Fartifactory+api+npm+team-a')
    expect(encodeRegistry('https://npm.example/registry'))
      .toBe(encodeRegistry('https://npm.example/registry/'))
  })
  test('keeps a repeated slash, which reaches the registry as a distinct path', () => {
    expect(encodeRegistry('https://npm.example/repo//'))
      .not.toBe(encodeRegistry('https://npm.example/repo/'))
    expect(encodeRegistry('https://npm.example//repo/'))
      .not.toBe(encodeRegistry('https://npm.example/repo/'))
    expect(encodeRegistry('https://npm.example/a//b/')).toBe('https%3A+npm.example%2Fa++b')
    // The repeated root is a distinct request path too: `https://npm.example/`,
    // `//` and `///` ask the registry for `/lodash`, `//lodash` and
    // `///lodash`, so all three are separate registries.
    const roots = ['https://npm.example/', 'https://npm.example//', 'https://npm.example///']
    expect(new Set(roots.map(encodeRegistry)).size).toBe(roots.length)
    expect(roots.map(encodeRegistry)).toStrictEqual([
      'https%3A+npm.example',
      'https%3A+npm.example%2F',
      'https%3A+npm.example%2F+',
    ])
  })
  test('same host, different paths never share a directory', () => {
    expect(encodeRegistry('https://releases.jfrog.io/artifactory/api/npm/team-a/'))
      .not.toBe(encodeRegistry('https://releases.jfrog.io/artifactory/api/npm/team-b/'))
  })
  test('escapes the delimiters', () => {
    const registries = [
      'https://repo.example/foo-bar/',
      'https://repo.example-foo/bar/',
      'https://repo.example/foo/',
      'https://repo.example_foo/',
      'https://nexus_npm/',
      'https://nexus/npm/',
      'https://npm.example/team/a/',
      'https://npm.example/team+a/',
      'https://npm.example/a%2Fb/',
      'https://npm.example/a%3Ab/',
    ]
    expect(new Set(registries.map(encodeRegistry)).size).toBe(registries.length)
    expect(encodeRegistry('https://npm.example/team+a/')).toBe('https%3A+npm.example%2Fteam%2Ba')
  })
  test('escapes filesystem and glob metacharacters', () => {
    expect(encodeRegistry('https://npm.example/a*b/')).toBe('https%3A+npm.example%2Fa%2Ab')
    expect(encodeRegistry('https://npm.example/a|b/')).toBe('https%3A+npm.example%2Fa%7Cb')
    // The URL parser reads a backslash in a special-scheme path as a separator.
    expect(encodeRegistry('https://npm.example/a\\b/')).toBe('https%3A+npm.example%2Fa+b')
    expect(encodeRegistry('https://npm.example/a%5Cb/'))
      .toBe('https%3A+npm.example%2Fa%255Cb%5Ff323ed1d3ec091d56df73b036cd0f4b4b20aa1bc06272a13cfd84f36894ed440')
  })
  test('no key can collide with one an earlier pnpm version wrote', () => {
    // Those keys were a bare URL host, which can never contain a `%`, and
    // every key now carries the scheme separator. A hostname may contain `_`,
    // so without that guarantee `https://nexus/npm/` would land on the
    // directory left behind for `https://nexus_npm/`.
    for (const registry of ['https://registry.npmjs.org/', 'http://localhost:4873/', 'https://nexus_npm/', 'https://nexus/npm/']) {
      expect(encodeRegistry(registry)).toContain('%')
    }
    expect(encodeRegistry('https://nexus_npm/')).toBe('https%3A+nexus_npm')
    expect(encodeRegistry('https://nexus/npm/')).toBe('https%3A+nexus%2Fnpm')
  })
  test('the scheme is part of the identity', () => {
    // http metadata can be rewritten in transit and must never be handed to a
    // resolution configured for https.
    expect(encodeRegistry('http://registry.example/repo/'))
      .not.toBe(encodeRegistry('https://registry.example/repo/'))
  })
  test('escapes a trailing period Win32 would strip', () => {
    expect(encodeRegistry('https://npm.example/foo./')).toBe('https%3A+npm.example%2Ffoo%2E')
    expect(encodeRegistry('https://npm.example/foo/')).toBe('https%3A+npm.example%2Ffoo')
  })
  test('hashes a mixed-case path', () => {
    expect(encodeRegistry('https://npm.example:8443/registry/A/'))
      .toBe('https%3A+npm.example+8443%2Fregistry+A%5Ff5296609e0eaab0d2f8fe3c4503600ed349a61793535d2c95505f0710b272e65')
    expect(encodeRegistry('https://npm.example:8443/registry/a/')).toBe('https%3A+npm.example+8443%2Fregistry+a')
  })
  test('hashes an oversized key', () => {
    const longPath = 'a'.repeat(300)
    const key = encodeRegistry(`https://npm.example/${longPath}/`)
    expect(key).toMatch(/^[0-9a-f]{64}$/)
    expect(key).not.toBe(encodeRegistry(`https://npm.example/${longPath}b/`))
  })
  test('rejects a registry that is not a URL with a host', () => {
    expect(() => encodeRegistry('invalid-url')).toThrow('Failed to parse registry URL "invalid-url"')
    expect(() => encodeRegistry('invalid-url')).toThrow(expect.objectContaining({ code: 'ERR_PNPM_INVALID_REGISTRY_URL' }))
    expect(() => encodeRegistry('file:///tmp/registry')).toThrow('has no host')
  })
  test('keeps credentials out of the whole error, not just its message', () => {
    let caught: unknown
    try {
      encodeRegistry('https://user:secret@')
    } catch (err: unknown) {
      caught = err
    }
    expect(caught).toBeDefined()
    expect((caught as Error).message).toBe('Failed to parse registry URL "https://": Invalid URL')
    // Node's ERR_INVALID_URL keeps the raw registry in `input`, so a preserved
    // cause would leak the password to anything that serializes the error.
    expect(util.inspect(caught, { depth: null })).not.toContain('secret')
  })
})

test('decodeRegistry', () => {
  expect(decodeRegistry('https%3A+registry.npmjs.org')).toBe('https://registry.npmjs.org/')
  expect(decodeRegistry('http%3A+localhost+4873')).toBe('http://localhost:4873/')
  expect(decodeRegistry('http%3A+%5B%3A%3A1%5D+8080')).toBe('http://[::1]:8080/')
  expect(decodeRegistry('https%3A+releases.jfrog.io%2Fartifactory+api+npm+team-a'))
    .toBe('https://releases.jfrog.io/artifactory/api/npm/team-a/')
  expect(decodeRegistry('https%3A+npm.example%2Fteam%2Ba')).toBe('https://npm.example/team+a/')
  expect(decodeRegistry('https%3A+npm.example+8443%2Fregistry+A%5Ff5296609e0eaab0d2f8fe3c4503600ed349a61793535d2c95505f0710b272e65'))
    .toBe('https://npm.example:8443/registry/A/')
  // Directories written before the scheme joined the key still label sensibly.
  expect(decodeRegistry('registry.npmjs.org')).toBe('registry.npmjs.org')
  expect(decodeRegistry('localhost+4873')).toBe('localhost:4873')
  expect(decodeRegistry('%FF')).toBe('%FF')
  expect(decodeRegistry('%not-a-key')).toBe('%not-a-key')
  expect(decodeRegistry('%not-a-key+8443')).toBe('%not-a-key+8443')
})

test('decodeRegistry is the exact inverse of encodeRegistry', () => {
  for (const registry of [
    'https://registry.npmjs.org/',
    'http://localhost:4873/',
    'http://[::1]:8080/',
    'https://npm.example:8443/registry/a/',
    'https://releases.jfrog.io/artifactory/api/npm/team-a/',
    'https://npm.example/team+a/',
    'https://npm.example//',
    'https://npm.example///',
    'https://npm.example/a//b/',
    'https://nexus_npm/',
  ]) {
    expect(decodeRegistry(encodeRegistry(registry))).toBe(registry)
  }
})

test('getPkgMirrorPath keeps same-host registries apart', () => {
  const mirrorPath = (registry: string): string => getPkgMirrorPath('/cache', ABBREVIATED_META_DIR, registry, 'is-positive')
  expect(mirrorPath('https://registry.npmjs.org/'))
    .toBe(path.join('/cache', ABBREVIATED_META_DIR, 'https%3A+registry.npmjs.org', 'is-positive.jsonl'))
  expect(mirrorPath('https://releases.jfrog.io/artifactory/api/npm/team-a/'))
    .not.toBe(mirrorPath('https://releases.jfrog.io/artifactory/api/npm/team-b/'))
})
