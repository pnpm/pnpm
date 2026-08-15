// A stand-in npm registry serving one `@pnpm/exe.<target>` version, for the
// tests around the Corepack entry point. Shared so the entry-point tests and
// the download tests describe the same registry.
import { Buffer } from 'node:buffer'
import { createHash } from 'node:crypto'
import http from 'node:http'
import zlib from 'node:zlib'

import { getBinCandidates, splitBinSpecifier } from '../native-binary.mjs'

export const VERSION = '12.0.0-test.0'
export const { packageName, binFile } = splitBinSpecifier(getBinCandidates()[0])

/**
 * Serve `payload` as the platform package's tarball, published under
 * `integrity(tarball)`.
 *
 * @param {object} opts
 * @param {Buffer | string} opts.payload Contents of the binary inside the tarball.
 * @param {(tarball: Buffer) => string} [opts.integrity] Checksum to publish.
 * @param {string} [opts.tarballUrl] URL to advertise, when the test needs the
 *   registry to hand back a URL of a different host than the one asked.
 * @returns {Promise<{url: string, close: () => Promise<void>, requestedPaths: string[]}>}
 */
export function startRegistry ({ payload, integrity = strongestIntegrity, tarballUrl }) {
  const tarball = zlib.gzipSync(tarArchive(`package/${binFile}`, Buffer.from(payload)))
  const published = integrity(tarball)
  const tarballPath = `/${packageName}/-/${VERSION}.tgz`
  const requestedPaths = []

  const server = http.createServer((req, res) => {
    requestedPaths.push(req.url)
    if (req.url === `/${packageName.replaceAll('/', '%2F')}/${VERSION}`) {
      res.writeHead(200, { 'content-type': 'application/json' })
      res.end(JSON.stringify({
        name: packageName,
        version: VERSION,
        dist: {
          integrity: published,
          tarball: tarballUrl ?? `http://127.0.0.1:${server.address().port}${tarballPath}`,
        },
      }))
    } else if (req.url === tarballPath) {
      res.writeHead(200, { 'content-type': 'application/octet-stream' })
      res.end(tarball)
    } else {
      res.writeHead(404).end()
    }
  })

  return new Promise((resolve) => {
    server.listen(0, '127.0.0.1', () => {
      resolve({
        url: `http://127.0.0.1:${server.address().port}`,
        close: () => new Promise((closed) => { server.close(closed) }),
        requestedPaths,
      })
    })
  })
}

export function strongestIntegrity (tarball) {
  return digest('sha512', tarball)
}

export function digest (algorithm, content) {
  return `${algorithm}-${createHash(algorithm).update(content).digest('base64')}`
}

/** A single-file ustar archive, terminated by the two empty blocks. */
export function tarArchive (name, content) {
  const header = Buffer.alloc(512)
  header.write(name, 0, 100)
  header.write('000755 \0', 100, 8)
  header.write('000000 \0', 108, 8)
  header.write('000000 \0', 116, 8)
  header.write(`${content.length.toString(8).padStart(11, '0')} `, 124, 12)
  header.write('00000000000 ', 136, 12)
  header.write('        ', 148, 8)
  header.write('0', 156, 1)
  header.write('ustar\x0000', 257, 8)
  const checksum = header.reduce((sum, byte) => sum + byte, 0)
  header.write(`${checksum.toString(8).padStart(6, '0')}\0 `, 148, 8)

  const padding = Buffer.alloc((512 - (content.length % 512)) % 512)
  return Buffer.concat([header, content, padding, Buffer.alloc(1024)])
}
