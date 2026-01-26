/// <reference path="../../../__typings__/index.d.ts"/>
import fs from 'fs'
import path from 'path'
import { PnpmError } from '@pnpm/error'
import { temporaryDirectory } from 'tempy'
import AdmZip from 'adm-zip'
import ssri from 'ssri'
import { downloadAndUnpackZip } from '@pnpm/fetching.binary-fetcher'

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
  })
})
