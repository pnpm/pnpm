import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { createHash, randomBytes } from 'node:crypto'
import fs from 'node:fs'
import https from 'node:https'
import os from 'node:os'
import path from 'node:path'
import { after, before, describe, test } from 'node:test'
import tls from 'node:tls'

import { downloadPnpmExecutable } from '../lib/index.js'
import { platformPackageName } from '../lib/platformPackageName.js'
import { useCertificateAuthorities } from '../lib/registry.js'

const VERSION = '99.0.0'
const EXECUTABLE = process.platform === 'win32' ? 'pnpm.exe' : 'pnpm'
const CONTENT = 'the pnpm executable, supposedly'
// The loopback certificate the Rust network tests use, issued for `127.0.0.1`
// by a CA no machine trusts. See `trusted_tls_server.rs` next to it for how it was made.
const TLS_FIXTURES = path.join(import.meta.dirname, '../../../crates/testing-utils/src/fixtures/tls')
const CA = fs.readFileSync(path.join(TLS_FIXTURES, 'ca.pem'), 'utf8')

const ADDS_CAS_AFTER_STARTUP = typeof tls.setDefaultCACertificates === 'function'

describe('a registry behind a private CA', { skip: !ADDS_CAS_AFTER_STARTUP && 'this Node.js cannot add a CA after startup' }, () => {
  let tmpDir: string
  let server: https.Server
  let registry: string
  let requests: string[] = []

  before(async () => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'get-pnpm-ca-test-'))
    const tarball = fs.readFileSync(packTarball(tmpDir))
    const packageName = platformPackageName({ major: 99, platform: process.platform, arch: process.arch, musl: false })
    server = https.createServer({
      cert: fs.readFileSync(path.join(TLS_FIXTURES, 'server.crt')),
      key: fs.readFileSync(path.join(TLS_FIXTURES, 'server.key')),
    }, (req, res) => {
      requests.push(req.url!)
      if (req.url === `/${packageName}/${VERSION}`) {
        res.writeHead(200, { 'content-type': 'application/json' })
        res.end(JSON.stringify({
          dist: {
            tarball: `${registry}${packageName}/-/tarball.tgz`,
            integrity: `sha512-${createHash('sha512').update(tarball).digest('base64')}`,
          },
        }))
        return
      }
      res.writeHead(200, { 'content-type': 'application/octet-stream' })
      res.end(tarball)
    })
    await new Promise<void>((resolve) => {
      server.listen(0, '127.0.0.1', resolve)
    })
    registry = `https://127.0.0.1:${(server.address() as { port: number }).port}/`
  })

  after(async () => {
    server.closeAllConnections()
    await new Promise<void>((resolve) => {
      server.close(() => {
        resolve()
      })
    })
    fs.rmSync(tmpDir, { recursive: true, force: true })
  })

  // A pooled connection or a resumed TLS session skips the certificate check,
  // so neither may carry over from a download that trusted the CA.
  const forgetConnections = (): void => {
    server.closeAllConnections()
    server.setTicketKeys(randomBytes(48))
  }

  // The signature is beside the point here, and has its own tests.
  const download = async (name: string, ca?: string[]): Promise<string> => {
    const destPath = path.join(tmpDir, name, EXECUTABLE)
    await downloadPnpmExecutable({ version: VERSION, registry, destPath, ca, verifySignature: false })
    return destPath
  }

  test('is refused without the CA', async () => {
    requests = []

    await assert.rejects(download('untrusted'), /Could not reach/)
    assert.deepEqual(requests, [])
  })

  test('is reached with the CA', async () => {
    const destPath = await download('trusted', [CA])

    assert.equal(fs.readFileSync(destPath, 'utf8'), CONTENT)
  })

  test('is refused again once the download is done', async () => {
    await download('restored', [CA])
    forgetConnections()
    requests = []

    await assert.rejects(download('after'), /Could not reach/)
    assert.deepEqual(requests, [])
  })

  test('keeps the CA until the last of two overlapping activations is released', async () => {
    const releaseFirst = useCertificateAuthorities([CA])
    const releaseSecond = useCertificateAuthorities([CA])
    releaseFirst()

    try {
      assert.equal(fs.readFileSync(await download('overlap'), 'utf8'), CONTENT)
    } finally {
      releaseSecond()
    }
    forgetConnections()
    await assert.rejects(download('overlap-released'), /Could not reach/)
  })
})

function packTarball (dir: string): string {
  const contentDir = path.join(dir, 'fixture', 'package')
  fs.mkdirSync(contentDir, { recursive: true })
  fs.writeFileSync(path.join(contentDir, EXECUTABLE), CONTENT)
  const tarball = path.join(dir, 'fixture.tgz')
  const { status, stderr } = spawnSync('tar', ['-czf', tarball, '-C', path.join(dir, 'fixture'), 'package'], { encoding: 'utf8' })
  if (status !== 0) throw new Error(`could not create the fixture tarball: ${stderr}`)
  return tarball
}
