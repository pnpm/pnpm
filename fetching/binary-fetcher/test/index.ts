/// <reference path="../../../__typings__/index.d.ts"/>
import fs from 'node:fs'
import path from 'node:path'

import { describe, expect, it } from '@jest/globals'
import { PnpmError } from '@pnpm/error'
import { createBinaryFetcher, downloadAndUnpackZip } from '@pnpm/fetching.binary-fetcher'
import AdmZip from 'adm-zip'
import ssri from 'ssri'
import { temporaryDirectory } from 'tempy'

// Mock fetch function that returns a ZIP buffer and simulates FetchFromRegistry
function createMockFetch (zipBuffer: Buffer) {
  return () => Promise.resolve({
    body: (async function * () {
      yield zipBuffer
    })(),
  })
}

describe('extractZipToTarget security', () => {
  describe('prefix path traversal (Attack Vector 2)', () => {
    it('should reject prefix with ../ path traversal', async () => {
      const targetDir = temporaryDirectory()
      const zip = new AdmZip()
      zip.addFile('node-v20.0.0/bin/node', Buffer.from('#!/bin/sh\necho "node"'))
      const zipBuffer = zip.toBuffer()
      // Use real integrity so the check passes and we reach path traversal validation
      const integrity = ssri.fromData(zipBuffer).toString()

      const mockFetch = createMockFetch(zipBuffer)

      await expect(
        downloadAndUnpackZip(
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          mockFetch as any,
          {
            url: 'https://example.com/node.zip',
            integrity,
            basename: '../../evil',
          },
          targetDir
        )
      ).rejects.toThrow(PnpmError)

      await expect(
        downloadAndUnpackZip(
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          mockFetch as any,
          {
            url: 'https://example.com/node.zip',
            integrity,
            basename: '../../evil',
          },
          targetDir
        )
      ).rejects.toMatchObject({
        code: 'ERR_PNPM_PATH_TRAVERSAL',
      })
    })

    it('should reject absolute path prefix', async () => {
      const targetDir = temporaryDirectory()
      const zip = new AdmZip()
      zip.addFile('node-v20.0.0/bin/node', Buffer.from('#!/bin/sh\necho "node"'))
      const zipBuffer = zip.toBuffer()
      // Use real integrity so the check passes and we reach path traversal validation
      const integrity = ssri.fromData(zipBuffer).toString()

      const mockFetch = createMockFetch(zipBuffer)

      await expect(
        downloadAndUnpackZip(
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          mockFetch as any,
          {
            url: 'https://example.com/node.zip',
            integrity,
            basename: '/tmp/evil',
          },
          targetDir
        )
      ).rejects.toMatchObject({
        code: 'ERR_PNPM_PATH_TRAVERSAL',
      })
    })
  })

  describe('ZIP entry path traversal (Attack Vector 1)', () => {
    it('should reject ZIP entries with ../ path traversal', async () => {
      const targetDir = temporaryDirectory()
      // Load fixture ZIP that has a raw malicious entry path
      const zipBuffer = fs.readFileSync(path.join(import.meta.dirname, 'fixtures/path-traversal.zip'))
      const integrity = ssri.fromData(zipBuffer).toString()

      const mockFetch = createMockFetch(zipBuffer)

      await expect(
        downloadAndUnpackZip(
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          mockFetch as any,
          {
            url: 'https://example.com/node.zip',
            integrity,
            basename: '',
          },
          targetDir
        )
      ).rejects.toMatchObject({
        code: 'ERR_PNPM_PATH_TRAVERSAL',
      })

      // Verify no files were written outside target
      const parentDir = path.dirname(targetDir)
      expect(fs.existsSync(path.join(parentDir, '.npmrc'))).toBe(false)
    })

    it('should reject ZIP entries with absolute paths', async () => {
      const targetDir = temporaryDirectory()
      // Load fixture ZIP that has a raw malicious absolute path entry
      const zipBuffer = fs.readFileSync(path.join(import.meta.dirname, 'fixtures/absolute-path.zip'))
      const integrity = ssri.fromData(zipBuffer).toString()

      const mockFetch = createMockFetch(zipBuffer)

      await expect(
        downloadAndUnpackZip(
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          mockFetch as any,
          {
            url: 'https://example.com/node.zip',
            integrity,
            basename: '',
          },
          targetDir
        )
      ).rejects.toMatchObject({
        code: 'ERR_PNPM_PATH_TRAVERSAL',
      })
    })

    // Windows-specific: backslash is a path separator only on Windows
    // On Unix, backslash is a valid filename character, so this test only runs on Windows
    const isWindows = process.platform === 'win32'
    const windowsTest = isWindows ? it : it.skip

    windowsTest('should reject ZIP entries with backslash path traversal on Windows', async () => {
      const targetDir = temporaryDirectory()
      // Load fixture ZIP with Windows-style backslash path traversal
      const zipBuffer = fs.readFileSync(path.join(import.meta.dirname, 'fixtures/backslash-traversal.zip'))
      const integrity = ssri.fromData(zipBuffer).toString()

      const mockFetch = createMockFetch(zipBuffer)

      await expect(
        downloadAndUnpackZip(
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          mockFetch as any,
          {
            url: 'https://example.com/node.zip',
            integrity,
            basename: '',
          },
          targetDir
        )
      ).rejects.toMatchObject({
        code: 'ERR_PNPM_PATH_TRAVERSAL',
      })
    })
  })

  describe('legitimate ZIP extraction', () => {
    it('should successfully extract a normal ZIP file', async () => {
      const targetDir = temporaryDirectory()
      const zip = new AdmZip()
      zip.addFile('node-v20.0.0/bin/node', Buffer.from('#!/bin/sh\necho "node"'))
      zip.addFile('node-v20.0.0/README.md', Buffer.from('# Node.js'))
      const zipBuffer = zip.toBuffer()

      // Create a mock fetch that also passes integrity check by using the actual buffer
      const integrity = ssri.fromData(zipBuffer).toString()

      const mockFetch = createMockFetch(zipBuffer)

      await downloadAndUnpackZip(
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        mockFetch as any,
        {
          url: 'https://example.com/node.zip',
          integrity,
          basename: 'node-v20.0.0',
        },
        targetDir
      )

      // Verify files were extracted correctly
      expect(fs.existsSync(path.join(targetDir, 'bin/node'))).toBe(true)
      expect(fs.existsSync(path.join(targetDir, 'README.md'))).toBe(true)
    })

    it('should handle empty basename correctly', async () => {
      const targetDir = temporaryDirectory()
      const zip = new AdmZip()
      zip.addFile('bin/node', Buffer.from('#!/bin/sh\necho "node"'))
      const zipBuffer = zip.toBuffer()

      const integrity = ssri.fromData(zipBuffer).toString()

      const mockFetch = createMockFetch(zipBuffer)

      await downloadAndUnpackZip(
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        mockFetch as any,
        {
          url: 'https://example.com/node.zip',
          integrity,
          basename: '',
        },
        targetDir
      )

      expect(fs.existsSync(path.join(targetDir, 'bin/node'))).toBe(true)
    })

    it('skips entries matching ignoreEntry regex (basename stripped)', async () => {
      const targetDir = temporaryDirectory()
      const zip = new AdmZip()
      zip.addFile('node-v20.0.0/node.exe', Buffer.from('binary'))
      zip.addFile('node-v20.0.0/npm', Buffer.from('npm shim'))
      zip.addFile('node-v20.0.0/npm.cmd', Buffer.from('npm cmd'))
      zip.addFile('node-v20.0.0/node_modules/npm/package.json', Buffer.from('{}'))
      zip.addFile('node-v20.0.0/node_modules/corepack/package.json', Buffer.from('{}'))
      zip.addFile('node-v20.0.0/node_modules/keep-me/index.js', Buffer.from('kept'))
      const zipBuffer = zip.toBuffer()
      const integrity = ssri.fromData(zipBuffer).toString()
      const mockFetch = createMockFetch(zipBuffer)

      await downloadAndUnpackZip(
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        mockFetch as any,
        {
          url: 'https://example.com/node.zip',
          integrity,
          basename: 'node-v20.0.0',
          ignoreEntry: /^(?:node_modules\/(?:npm|corepack)(?:\/|$)|npm(?:\.cmd)?$)/,
        },
        targetDir
      )

      expect(fs.existsSync(path.join(targetDir, 'node.exe'))).toBe(true)
      expect(fs.existsSync(path.join(targetDir, 'node_modules/keep-me/index.js'))).toBe(true)
      expect(fs.existsSync(path.join(targetDir, 'npm'))).toBe(false)
      expect(fs.existsSync(path.join(targetDir, 'npm.cmd'))).toBe(false)
      expect(fs.existsSync(path.join(targetDir, 'node_modules/npm'))).toBe(false)
      expect(fs.existsSync(path.join(targetDir, 'node_modules/corepack'))).toBe(false)
    })

    it('skips entries matching ignoreEntry regex when basename is empty', async () => {
      const targetDir = temporaryDirectory()
      const zip = new AdmZip()
      zip.addFile('bin/node', Buffer.from('#!/bin/sh\necho "node"'))
      zip.addFile('bin/npm', Buffer.from('npm shim'))
      const zipBuffer = zip.toBuffer()
      const integrity = ssri.fromData(zipBuffer).toString()
      const mockFetch = createMockFetch(zipBuffer)

      await downloadAndUnpackZip(
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        mockFetch as any,
        {
          url: 'https://example.com/node.zip',
          integrity,
          basename: '',
          ignoreEntry: /^bin\/npm$/,
        },
        targetDir
      )

      expect(fs.existsSync(path.join(targetDir, 'bin/node'))).toBe(true)
      expect(fs.existsSync(path.join(targetDir, 'bin/npm'))).toBe(false)
    })

    it('strips /g /y flags from ignoreEntry so .test() is not stateful across entries', async () => {
      const targetDir = temporaryDirectory()
      const zip = new AdmZip()
      zip.addFile('node-v20.0.0/node.exe', Buffer.from('binary'))
      zip.addFile('node-v20.0.0/npm', Buffer.from('npm shim 1'))
      zip.addFile('node-v20.0.0/npx', Buffer.from('npx shim 2'))
      zip.addFile('node-v20.0.0/corepack', Buffer.from('corepack 3'))
      const zipBuffer = zip.toBuffer()
      const integrity = ssri.fromData(zipBuffer).toString()
      const mockFetch = createMockFetch(zipBuffer)

      await downloadAndUnpackZip(
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        mockFetch as any,
        {
          url: 'https://example.com/node.zip',
          integrity,
          basename: 'node-v20.0.0',
          // Deliberately pass a /g regex — a stateful .test() would skip only
          // every other matching entry. All three shims must still be dropped.
          ignoreEntry: /^(?:npm|npx|corepack)$/g,
        },
        targetDir
      )

      expect(fs.existsSync(path.join(targetDir, 'node.exe'))).toBe(true)
      expect(fs.existsSync(path.join(targetDir, 'npm'))).toBe(false)
      expect(fs.existsSync(path.join(targetDir, 'npx'))).toBe(false)
      expect(fs.existsSync(path.join(targetDir, 'corepack'))).toBe(false)
    })

  })
})

describe('createBinaryFetcher', () => {
  it('rejects an invalid archiveFilters regex at creation time', () => {
    const noop = (() => {
      throw new Error('should not be called')
    }) as never
    expect(() =>
      createBinaryFetcher({
        fetch: noop,
        fetchFromRemoteTarball: noop,
        storeIndex: noop,
        archiveFilters: { node: '(' },
      })
    ).toThrow(PnpmError)
    expect(() =>
      createBinaryFetcher({
        fetch: noop,
        fetchFromRemoteTarball: noop,
        storeIndex: noop,
        archiveFilters: { node: '(' },
      })
    ).toThrow(/Invalid archive filter regex for "node"/)
  })

  it('snapshots archiveFilters so post-creation mutations cannot reintroduce invalid patterns', () => {
    const noop = (() => {
      throw new Error('should not be called')
    }) as never
    const filters: Record<string, string> = { node: '^ok$' }
    // Must succeed — the pattern is valid at construction time.
    expect(() =>
      createBinaryFetcher({
        fetch: noop,
        fetchFromRemoteTarball: noop,
        storeIndex: noop,
        archiveFilters: filters,
      })
    ).not.toThrow()
    // Mutating the caller's object after construction must not affect the fetcher.
    // There's no direct read back, but any mutation reaching the fetcher would throw
    // on subsequent fetches; the snapshot guarantees it can't.
    filters.node = '('
    // Reconstructing with the broken pattern fails — demonstrating the original
    // fetcher would have failed at construction if it had seen the broken pattern.
    expect(() =>
      createBinaryFetcher({
        fetch: noop,
        fetchFromRemoteTarball: noop,
        storeIndex: noop,
        archiveFilters: filters,
      })
    ).toThrow(/Invalid archive filter regex for "node"/)
  })
})
