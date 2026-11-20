import path from 'node:path'
import zlib from 'node:zlib'

import { tempDir } from '@pnpm/prepare'
import { DEFAULT_OPTS as BASE_OPTS, REGISTRY_URL } from '@pnpm/testing.command-defaults'

export { REGISTRY_URL }

const tmp = tempDir()

export const DEFAULT_OPTS = {
  ...BASE_OPTS,
  bail: false,
  extraBinPaths: [],
  supportedArchitectures: {
    os: ['current'],
    cpu: ['current'],
    libc: ['current'],
  },
}

export const DLX_DEFAULT_OPTS = {
  ...BASE_OPTS,
  bail: false,
  cacheDir: path.join(tmp, 'cache'),
  dlxCacheMaxAge: Infinity,
  extraBinPaths: [],
  lock: true,
  pnpmfile: ['.pnpmfile.cjs'],
  storeDir: path.join(tmp, 'store'),
  symlink: true,
  supportedArchitectures: {
    os: ['current'],
    cpu: ['current'],
    libc: ['current'],
  },
  workspaceConcurrency: 1,
}

export function createTarballBuffer (entries: Array<[string, string]>): Buffer {
  const blocks: Buffer[] = []
  for (const [entryPath, content] of entries) {
    const body = Buffer.from(content, 'utf8')
    const header = Buffer.alloc(512, 0)
    header.write(entryPath, 0, 100, 'utf8')
    header.write('0000755\0', 100, 8, 'ascii')
    header.write('0000000\0', 108, 8, 'ascii')
    header.write('0000000\0', 116, 8, 'ascii')
    header.write(`${body.length.toString(8).padStart(11, '0')}\0`, 124, 12, 'ascii')
    header.write('00000000000\0', 136, 12, 'ascii')
    header.fill(0x20, 148, 156)
    header.write('0', 156, 1, 'ascii')
    header.write('ustar\0', 257, 6, 'ascii')
    header.write('00', 263, 2, 'ascii')
    let sum = 0
    for (const byte of header) sum += byte
    header.write(`${sum.toString(8).padStart(6, '0')}\0 `, 148, 8, 'ascii')
    blocks.push(header, body)
    const remainder = body.length % 512
    if (remainder > 0) {
      blocks.push(Buffer.alloc(512 - remainder, 0))
    }
  }
  blocks.push(Buffer.alloc(1024, 0))
  return zlib.gzipSync(Buffer.concat(blocks))
}
