import fs from 'node:fs'
import path from 'node:path'

import { describe, expect, test, afterEach } from '@jest/globals'
import { temporaryDirectory } from 'tempy'

import {
  RegistryMetadataCache,
  closeAllRegistryMetadataCaches,
} from '@pnpm/resolving.registry-metadata-cache'

describe('RegistryMetadataCache', () => {
  afterEach(() => {
    closeAllRegistryMetadataCaches()
  })

  test('stores and retrieves metadata', () => {
    const cacheDir = temporaryDirectory()
    const cache = new RegistryMetadataCache(cacheDir)

    try {
      const meta = {
        name: 'is-positive',
        'dist-tags': { latest: '3.1.0' },
        versions: {
          '1.0.0': { version: '1.0.0', name: 'is-positive' },
          '2.0.0': { version: '2.0.0', name: 'is-positive' },
        },
      } as any

      expect(cache.get('is-positive', 'https://registry.npmjs.org/')).toBeUndefined()

      cache.set('is-positive', 'https://registry.npmjs.org/', meta)

      const result = cache.get('is-positive', 'https://registry.npmjs.org/')
      expect(result).toBeDefined()
      expect(result!.name).toBe('is-positive')
      expect(result!['dist-tags'].latest).toBe('3.1.0')
      expect(Object.keys(result!.versions)).toHaveLength(2)
    } finally {
      cache.close()
    }
  })

  test('stores and retrieves headers', () => {
    const cacheDir = temporaryDirectory()
    const cache = new RegistryMetadataCache(cacheDir)

    try {
      const meta = {
        name: 'is-positive',
        'dist-tags': { latest: '3.1.0' },
        versions: {},
        etag: '"abc123"',
        modified: 'Wed, 21 Oct 2015 07:28:00 GMT',
      } as any

      cache.set('is-positive', 'https://registry.npmjs.org/', meta)

      const headers = cache.getHeaders('is-positive', 'https://registry.npmjs.org/')
      expect(headers).toBeDefined()
      expect(headers!.etag).toBe('"abc123"')
      expect(headers!.modified).toBe('Wed, 21 Oct 2015 07:28:00 GMT')
    } finally {
      cache.close()
    }
  })

  test('returns undefined for missing keys', () => {
    const cacheDir = temporaryDirectory()
    const cache = new RegistryMetadataCache(cacheDir)

    try {
      expect(cache.has('non-existent', 'https://registry.npmjs.org/')).toBe(false)
      expect(cache.get('non-existent', 'https://registry.npmjs.org/')).toBeUndefined()
      expect(cache.getHeaders('non-existent', 'https://registry.npmjs.org/')).toBeUndefined()
    } finally {
      cache.close()
    }
  })

  test('persists data across instances', () => {
    const cacheDir = temporaryDirectory()
    const cache1 = new RegistryMetadataCache(cacheDir)

    const meta = {
      name: 'is-positive',
      'dist-tags': { latest: '3.1.0' },
      versions: { '1.0.0': { version: '1.0.0', name: 'is-positive' } },
      etag: '"abc123"',
      modified: 'Wed, 21 Oct 2015 07:28:00 GMT',
    } as any

    cache1.set('is-positive', 'https://registry.npmjs.org/', meta)
    cache1.close()

    // Create new instance pointing to same directory
    const cache2 = new RegistryMetadataCache(cacheDir)
    try {
      expect(cache2.has('is-positive', 'https://registry.npmjs.org/')).toBe(true)

      const result = cache2.get('is-positive', 'https://registry.npmjs.org/')
      expect(result).toBeDefined()
      expect(result!.name).toBe('is-positive')
      expect(result!['dist-tags'].latest).toBe('3.1.0')

      const headers = cache2.getHeaders('is-positive', 'https://registry.npmjs.org/')
      expect(headers).toBeDefined()
      expect(headers!.etag).toBe('"abc123"')
    } finally {
      cache2.close()
    }
  })

  test('handles different registries', () => {
    const cacheDir = temporaryDirectory()
    const cache = new RegistryMetadataCache(cacheDir)

    try {
      const npmMeta = {
        name: 'is-positive',
        'dist-tags': { latest: '3.1.0' },
        versions: {},
      } as any

      const privateMeta = {
        name: 'is-positive',
        'dist-tags': { latest: '1.0.0' },
        versions: {},
      } as any

      cache.set('is-positive', 'https://registry.npmjs.org/', npmMeta)
      cache.set('is-positive', 'https://private.registry.com/', privateMeta)

      const npmResult = cache.get('is-positive', 'https://registry.npmjs.org/')
      expect(npmResult).toBeDefined()
      expect(npmResult!['dist-tags'].latest).toBe('3.1.0')

      const privateResult = cache.get('is-positive', 'https://private.registry.com/')
      expect(privateResult).toBeDefined()
      expect(privateResult!['dist-tags'].latest).toBe('1.0.0')

      // Verify they're independent
      expect(cache.has('is-positive', 'https://registry.npmjs.org/')).toBe(true)
      expect(cache.has('is-positive', 'https://private.registry.com/')).toBe(true)
      expect(cache.has('is-positive', 'https://other.registry.com/')).toBe(false)
    } finally {
      cache.close()
    }
  })

  test('has() returns true for existing keys', () => {
    const cacheDir = temporaryDirectory()
    const cache = new RegistryMetadataCache(cacheDir)

    try {
      const meta = { name: 'is-positive', 'dist-tags': {}, versions: {} } as any

      expect(cache.has('is-positive', 'https://registry.npmjs.org/')).toBe(false)

      cache.set('is-positive', 'https://registry.npmjs.org/', meta)

      expect(cache.has('is-positive', 'https://registry.npmjs.org/')).toBe(true)
    } finally {
      cache.close()
    }
  })
})
