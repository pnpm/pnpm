import { describe, expect, it } from '@jest/globals'
import { decodeResponse, type ResponseMetadata } from '@pnpm/registry.client'
import type { DepPath } from '@pnpm/types'

describe('protocol decoding', () => {
  it('decodes a response with metadata and files', async () => {
    // Build a response buffer manually matching the binary protocol spec
    const metadata: ResponseMetadata = {
      lockfile: {
        lockfileVersion: '9.0',
        importers: {
          '.': {
            specifiers: {},
            dependencies: { 'is-positive': '1.0.0' },
          },
        },
        packages: {
          '/is-positive/1.0.0': {
            resolution: { integrity: 'sha512-test123' },
          },
        },
      } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
      packageFiles: {
        '/is-positive/1.0.0': {
          integrity: 'sha512-test123',
          algo: 'sha512',
          files: {
            'index.js': { digest: 'a'.repeat(128), size: 11, mode: 0o644 },
          },
        },
      },
      missingDigests: ['a'.repeat(128)],
      stats: {
        totalPackages: 1,
        alreadyInStore: 0,
        packagesToFetch: 1,
        filesInNewPackages: 1,
        filesAlreadyInCafs: 0,
        filesToDownload: 1,
        downloadBytes: 11,
      },
    }

    const jsonBuf = Buffer.from(JSON.stringify(metadata), 'utf-8')
    const parts: Buffer[] = []

    // 4-byte JSON length
    const lenBuf = Buffer.alloc(4)
    lenBuf.writeUInt32BE(jsonBuf.length, 0)
    parts.push(lenBuf)

    // JSON metadata
    parts.push(jsonBuf)

    // File entry: 64-byte digest + 4-byte size + 1-byte mode + content
    const fileContent = Buffer.from('hello world')
    const digestBuf = Buffer.from('a'.repeat(128), 'hex') // 64 bytes
    const sizeBuf = Buffer.alloc(4)
    sizeBuf.writeUInt32BE(fileContent.length, 0)
    const modeBuf = Buffer.alloc(1, 0x00) // non-executable

    parts.push(digestBuf)
    parts.push(sizeBuf)
    parts.push(modeBuf)
    parts.push(fileContent)

    // End marker: 64 zero bytes
    parts.push(Buffer.alloc(64, 0))

    const responseBuf = Buffer.concat(parts)

    // Decode
    async function * toStream (): AsyncIterable<Buffer> {
      yield responseBuf
    }

    const { metadata: decoded, files } = await decodeResponse(toStream())

    // Verify metadata
    expect(decoded.stats.totalPackages).toBe(1)
    expect(decoded.stats.filesToDownload).toBe(1)
    expect(decoded.missingDigests).toEqual(['a'.repeat(128)])
    expect(decoded.packageFiles['/is-positive/1.0.0'].integrity).toBe('sha512-test123')
    expect(decoded.lockfile.packages?.['/is-positive/1.0.0' as DepPath]).toBeTruthy()

    // Verify file
    expect(files).toHaveLength(1)
    expect(files[0].digest).toBe('a'.repeat(128))
    expect(files[0].size).toBe(11)
    expect(files[0].executable).toBe(false)
    expect(files[0].content.toString()).toBe('hello world')
  })

  it('decodes a response with executable files', async () => {
    const metadata: ResponseMetadata = {
      lockfile: { lockfileVersion: '9.0', importers: {} } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
      packageFiles: {},
      missingDigests: ['b'.repeat(128)],
      stats: {
        totalPackages: 0,
        alreadyInStore: 0,
        packagesToFetch: 0,
        filesInNewPackages: 0,
        filesAlreadyInCafs: 0,
        filesToDownload: 1,
        downloadBytes: 4,
      },
    }

    const jsonBuf = Buffer.from(JSON.stringify(metadata), 'utf-8')
    const parts: Buffer[] = []

    const lenBuf = Buffer.alloc(4)
    lenBuf.writeUInt32BE(jsonBuf.length, 0)
    parts.push(lenBuf)
    parts.push(jsonBuf)

    // Executable file
    const digestBuf = Buffer.from('b'.repeat(128), 'hex')
    const sizeBuf = Buffer.alloc(4)
    const content = Buffer.from('#!/bin/sh\n')
    sizeBuf.writeUInt32BE(content.length, 0)
    parts.push(digestBuf)
    parts.push(sizeBuf)
    parts.push(Buffer.from([0x01])) // executable
    parts.push(content)

    parts.push(Buffer.alloc(64, 0)) // end marker

    async function * toStream (): AsyncIterable<Buffer> {
      yield Buffer.concat(parts)
    }

    const { files } = await decodeResponse(toStream())

    expect(files).toHaveLength(1)
    expect(files[0].executable).toBe(true)
    expect(files[0].content.toString()).toBe('#!/bin/sh\n')
  })

  it('decodes a response with no files', async () => {
    const metadata: ResponseMetadata = {
      lockfile: { lockfileVersion: '9.0', importers: {} } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
      packageFiles: {},
      missingDigests: [],
      stats: {
        totalPackages: 5,
        alreadyInStore: 5,
        packagesToFetch: 0,
        filesInNewPackages: 0,
        filesAlreadyInCafs: 0,
        filesToDownload: 0,
        downloadBytes: 0,
      },
    }

    const jsonBuf = Buffer.from(JSON.stringify(metadata), 'utf-8')
    const parts: Buffer[] = []

    const lenBuf = Buffer.alloc(4)
    lenBuf.writeUInt32BE(jsonBuf.length, 0)
    parts.push(lenBuf)
    parts.push(jsonBuf)
    parts.push(Buffer.alloc(64, 0)) // end marker immediately

    async function * toStream (): AsyncIterable<Buffer> {
      yield Buffer.concat(parts)
    }

    const { metadata: decoded, files } = await decodeResponse(toStream())

    expect(files).toHaveLength(0)
    expect(decoded.stats.alreadyInStore).toBe(5)
  })
})
