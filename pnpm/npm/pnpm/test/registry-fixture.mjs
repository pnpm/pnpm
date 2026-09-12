// A stand-in npm registry serving one `@pnpm/exe.<target>` version, for the
// tests around the Corepack entry point.
//
// The download verifies npm's signature over the checksum, so the fixture signs
// with a key of its own and the tests hand that key to the entry point through
// `COREPACK_INTEGRITY_KEYS` — the same way a user on a registry that re-signs
// does, and the same knob Corepack itself reads.
import { Buffer } from 'node:buffer'
import { createHash, createSign, generateKeyPairSync } from 'node:crypto'
import http from 'node:http'
import process from 'node:process'
import zlib from 'node:zlib'

import { isMusl, platformPackageName } from 'get-pnpm'

export const VERSION = '99.0.0'
export const packageName = platformPackageName({
  major: 99,
  platform: process.platform,
  arch: process.arch,
  musl: isMusl(),
})
export const binFile = process.platform === 'win32' ? 'pnpm.exe' : 'pnpm'

const { privateKey, publicKey } = generateKeyPairSync('ec', { namedCurve: 'prime256v1' })

/** What `COREPACK_INTEGRITY_KEYS` has to hold for a fixture download to verify. */
export const INTEGRITY_KEYS = JSON.stringify({
  npm: [{
    keyid: 'SHA256:fixture-key',
    key: publicKey.export({ type: 'spki', format: 'der' }).toString('base64'),
    expires: null,
  }],
})

/**
 * Serve `payload` as the platform package's tarball.
 *
 * @param {object} opts
 * @param {Buffer | string} opts.payload Contents of the binary inside the tarball.
 * @param {boolean} [opts.tamper] Serve bytes the published checksum does not cover.
 * @param {boolean} [opts.unsigned] Publish the package without a signature.
 * @returns {Promise<{url: string, close: () => Promise<void>}>}
 */
export function startRegistry ({ payload, tamper = false, unsigned = false }) {
  const tarball = zlib.gzipSync(tarArchive(`package/${binFile}`, Buffer.from(payload)))
  const served = tamper ? Buffer.concat([tarball, Buffer.from('tampered')]) : tarball
  const integrity = `sha512-${createHash('sha512').update(tarball).digest('base64')}`
  const tarballPath = `/${packageName}/-/${VERSION}.tgz`

  const server = http.createServer((req, res) => {
    if (req.url === `/${packageName}/${VERSION}`) {
      res.writeHead(200, { 'content-type': 'application/json' })
      res.end(JSON.stringify({
        name: packageName,
        version: VERSION,
        dist: {
          integrity,
          tarball: `http://127.0.0.1:${server.address().port}${tarballPath}`,
          signatures: unsigned ? [] : [{
            keyid: 'SHA256:fixture-key',
            sig: createSign('SHA256')
              .update(`${packageName}@${VERSION}:${integrity}`)
              .sign(privateKey, 'base64'),
          }],
        },
      }))
    } else if (req.url === tarballPath) {
      res.writeHead(200, { 'content-type': 'application/octet-stream' })
      res.end(served)
    } else {
      res.writeHead(404).end()
    }
  })

  return new Promise((resolve) => {
    server.listen(0, '127.0.0.1', () => {
      resolve({
        url: `http://127.0.0.1:${server.address().port}`,
        close: () => new Promise((closed) => {
          server.close(closed)
          // `close` alone waits out an idle keep-alive connection; absent
          // before Node.js 18.2, which the package still claims to support.
          server.closeAllConnections?.()
        }),
      })
    })
  })
}

/** A single-file ustar archive, terminated by the two empty blocks. */
function tarArchive (name, content) {
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
