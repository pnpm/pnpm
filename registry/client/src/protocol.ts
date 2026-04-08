import type { LockfileObject } from '@pnpm/lockfile.types'

export interface PackageFilesInfo {
  integrity: string
  algo: string
  files: Record<string, { digest: string, size: number, mode: number }>
}

export interface ResponseMetadata {
  lockfile: LockfileObject
  packageFiles: Record<string, PackageFilesInfo>
  missingDigests: string[]
  stats: {
    totalPackages: number
    alreadyInStore: number
    packagesToFetch: number
    filesInNewPackages: number
    filesAlreadyInCafs: number
    filesToDownload: number
    downloadBytes: number
  }
}

export interface DecodedFile {
  digest: string
  size: number
  executable: boolean
  content: Buffer
}

/**
 * Decode a pnpm-registry binary response stream.
 *
 * Format:
 *   [4 bytes: JSON metadata length (big-endian uint32)]
 *   [N bytes: JSON metadata]
 *   [file entries...]
 *   [64 zero bytes: end marker]
 *
 * Each file entry:
 *   [64 bytes: SHA-512 digest, raw binary]
 *   [4 bytes: file size (big-endian uint32)]
 *   [1 byte: 0x00=regular, 0x01=executable]
 *   [N bytes: file content]
 */
export async function decodeResponse (stream: AsyncIterable<Buffer>): Promise<{
  metadata: ResponseMetadata
  files: DecodedFile[]
}> {
  const buffer = await collectStream(stream)
  let offset = 0

  // 1. Read JSON metadata length
  const jsonLength = buffer.readUInt32BE(offset)
  offset += 4

  // 2. Read and parse JSON metadata
  const jsonBuf = buffer.subarray(offset, offset + jsonLength)
  offset += jsonLength
  const metadata: ResponseMetadata = JSON.parse(jsonBuf.toString('utf-8'))

  // 3. Read file entries
  const files: DecodedFile[] = []
  const END_MARKER = Buffer.alloc(64, 0)

  while (offset < buffer.length) {
    // Check for end marker
    const possibleEnd = buffer.subarray(offset, offset + 64)
    if (possibleEnd.length === 64 && possibleEnd.equals(END_MARKER)) {
      break
    }

    // Digest: 64 bytes raw binary → hex string
    const digestBuf = buffer.subarray(offset, offset + 64)
    offset += 64
    const digest = digestBuf.toString('hex')

    // Size: 4 bytes big-endian uint32
    const size = buffer.readUInt32BE(offset)
    offset += 4

    // Mode: 1 byte
    const executable = buffer[offset] === 0x01
    offset += 1

    // Content: [size] bytes
    const content = Buffer.from(buffer.subarray(offset, offset + size))
    offset += size

    files.push({ digest, size, executable, content })
  }

  return { metadata, files }
}

async function collectStream (stream: AsyncIterable<Buffer>): Promise<Buffer> {
  const chunks: Buffer[] = []
  for await (const chunk of stream) {
    chunks.push(chunk)
  }
  return Buffer.concat(chunks)
}
