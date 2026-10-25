import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import fs from 'node:fs'
import http from 'node:http'
import os from 'node:os'
import path from 'node:path'
import { after, before, beforeEach, describe, test } from 'node:test'

import { downloadTarball, type VersionMeta } from '../lib/registry.js'

describe('stalled tarball download', () => {
  let server: http.Server
  let origin: string
  /** Tarball requests left to leave unanswered, without even a status line. */
  let silencesRemaining: number
  /** Tarball requests left to answer with half the body and then nothing more. */
  let stallsRemaining: number
  let requests: number
  const body = Buffer.from('x'.repeat(4096))
  const integrity = `sha512-${createHash('sha512').update(body).digest('base64')}`
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'get-pnpm-stall-'))

  before(async () => {
    server = http.createServer((req, res) => {
      requests++
      if (silencesRemaining > 0) {
        silencesRemaining--
        return
      }
      res.writeHead(200, { 'content-type': 'application/octet-stream', 'content-length': body.length })
      if (stallsRemaining > 0) {
        stallsRemaining--
        // Left open, like a registry that stopped sending partway through.
        res.write(body.subarray(0, body.length >> 1))
        return
      }
      res.end(body)
    })
    await new Promise<void>((resolve) => {
      server.listen(0, '127.0.0.1', resolve)
    })
    origin = `http://127.0.0.1:${(server.address() as { port: number }).port}`
  })

  after(async () => {
    server.closeAllConnections()
    await new Promise<void>((resolve) => {
      server.close(() => {
        resolve()
      })
    })
    fs.rmSync(dir, { recursive: true, force: true })
  })

  beforeEach(() => {
    silencesRemaining = 0
    stallsRemaining = 0
    requests = 0
  })

  test('retries a tarball download that stopped receiving data partway through the body', async () => {
    stallsRemaining = 1
    const dest = path.join(dir, 'stalled-body.tgz')

    await downloadTarball(meta(`${origin}/pkg.tgz`), dest, { stallTimeoutMs: 200 })

    assert.deepEqual(fs.readFileSync(dest), body)
    assert.equal(requests, 2)
  })

  test('retries a tarball download whose response never arrived', async () => {
    silencesRemaining = 1
    const dest = path.join(dir, 'silent.tgz')

    await downloadTarball(meta(`${origin}/pkg.tgz`), dest, { stallTimeoutMs: 200 })

    assert.deepEqual(fs.readFileSync(dest), body)
    assert.equal(requests, 2)
  })

  function meta (tarball: string): VersionMeta {
    return { dist: { tarball, integrity } }
  }
})
