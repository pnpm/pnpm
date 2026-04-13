import { describe, expect, it } from '@jest/globals'
import { decodeResponse, type ResponseMetadata } from '@pnpm/agent.client'
import type { DepPath } from '@pnpm/types'

describe('protocol decoding', () => {
  it('decodes a response with metadata and files', async () => {
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
      missingFiles: [
        { digest: 'a'.repeat(128), size: 11, executable: false },
      ],
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

    const lenBuf = Buffer.alloc(4)
    lenBuf.writeUInt32BE(jsonBuf.length, 0)
    parts.push(lenBuf)
    parts.push(jsonBuf)

    // File entry: 64-byte digest + 4-byte size + 1-byte mode + content
    const fileContent = Buffer.from('hello world')
    const digestBuf = Buffer.from('a'.repeat(128), 'hex')
    const sizeBuf = Buffer.alloc(4)
    sizeBuf.writeUInt32BE(fileContent.length, 0)
    const modeBuf = Buffer.alloc(1, 0x00)

    parts.push(digestBuf)
    parts.push(sizeBuf)
    parts.push(modeBuf)
    parts.push(fileContent)

    parts.push(Buffer.alloc(64, 0))

    const responseBuf = Buffer.concat(parts)

    async function * toStream (): AsyncIterable<Buffer> {
      yield responseBuf
    }

    const { metadata: decoded, files } = await decodeResponse(toStream())

    expect(decoded.stats.totalPackages).toBe(1)
    expect(decoded.stats.filesToDownload).toBe(1)
    expect(decoded.missingFiles).toHaveLength(1)
    expect(decoded.missingFiles[0].digest).toBe('a'.repeat(128))
    expect(decoded.lockfile.packages?.['/is-positive/1.0.0' as DepPath]).toBeTruthy()

    expect(files).toHaveLength(1)
    expect(files[0].digest).toBe('a'.repeat(128))
    expect(files[0].size).toBe(11)
    expect(files[0].executable).toBe(false)
    expect(files[0].content.toString()).toBe('hello world')
  })

  it('decodes a response with executable files', async () => {
    const metadata: ResponseMetadata = {
      lockfile: { lockfileVersion: '9.0', importers: {} } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
      missingFiles: [
        { digest: 'b'.repeat(128), size: 10, executable: true },
      ],
      stats: {
        totalPackages: 0,
        alreadyInStore: 0,
        packagesToFetch: 0,
        filesInNewPackages: 0,
        filesAlreadyInCafs: 0,
        filesToDownload: 1,
        downloadBytes: 10,
      },
    }

    const jsonBuf = Buffer.from(JSON.stringify(metadata), 'utf-8')
    const parts: Buffer[] = []

    const lenBuf = Buffer.alloc(4)
    lenBuf.writeUInt32BE(jsonBuf.length, 0)
    parts.push(lenBuf)
    parts.push(jsonBuf)

    const digestBuf = Buffer.from('b'.repeat(128), 'hex')
    const content = Buffer.from('#!/bin/sh\n')
    const sizeBuf = Buffer.alloc(4)
    sizeBuf.writeUInt32BE(content.length, 0)
    parts.push(digestBuf)
    parts.push(sizeBuf)
    parts.push(Buffer.from([0x01])) // executable
    parts.push(content)

    parts.push(Buffer.alloc(64, 0))

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
      missingFiles: [],
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
    parts.push(Buffer.alloc(64, 0))

    async function * toStream (): AsyncIterable<Buffer> {
      yield Buffer.concat(parts)
    }

    const { metadata: decoded, files } = await decodeResponse(toStream())

    expect(files).toHaveLength(0)
    expect(decoded.stats.alreadyInStore).toBe(5)
  })
})
