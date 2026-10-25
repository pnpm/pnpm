import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import fs from 'node:fs'
import http from 'node:http'
import os from 'node:os'
import path from 'node:path'
import { after, before, describe, test } from 'node:test'

import { downloadTarball, fetchVersionMeta, type VersionMeta } from '../lib/registry.js'

const REDIRECTS = new Map([
  ['/npm/redirect-out.tgz', '/other/pkg.tgz'],
  ['/npm/redirect-in.tgz', '/npm/final.tgz'],
  ['/npm/meta-redirect/1.0.0', '/other/meta.json'],
])

describe('registry credential scope', () => {
  let server: http.Server
  let origin: string
  let authorizations: Array<string | undefined>
  const body = Buffer.from('scoped tarball')
  const integrity = `sha512-${createHash('sha512').update(body).digest('base64')}`
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'get-pnpm-auth-scope-'))

  before(async () => {
    authorizations = []
    server = http.createServer((req, res) => {
      authorizations.push(req.headers.authorization)
      const location = REDIRECTS.get(req.url!)
      if (location != null) {
        res.writeHead(302, { location })
        res.end()
        return
      }
      if (req.url === '/other/meta.json') {
        res.writeHead(200, { 'content-type': 'application/json' })
        res.end(JSON.stringify({ dist: { tarball: `${origin}/npm/pkg.tgz`, integrity } }))
        return
      }
      res.writeHead(200, { 'content-type': 'application/octet-stream' })
      res.end(body)
    })
    await new Promise<void>((resolve) => {
      server.listen(0, '127.0.0.1', resolve)
    })
    origin = `http://127.0.0.1:${(server.address() as { port: number }).port}`
  })

  after(async () => {
    await new Promise<void>((resolve) => {
      server.close(() => {
        resolve()
      })
    })
    fs.rmSync(dir, { recursive: true, force: true })
  })

  test('sends scoped credentials to a tarball inside the configured path', async () => {
    authorizations = []
    await downloadTarball(meta(`${origin}/npm/pkg.tgz`), path.join(dir, 'in-scope.tgz'), {
      registry: `${origin}/npm/`,
      headers: { authorization: 'Bearer scoped-token' },
      headersPath: '/npm/',
    })

    assert.deepEqual(authorizations, ['Bearer scoped-token'])
  })

  test('withholds scoped credentials from a same-origin tarball outside the configured path', async () => {
    authorizations = []
    await downloadTarball(meta(`${origin}/other/pkg.tgz`), path.join(dir, 'out-of-scope.tgz'), {
      registry: `${origin}/npm/`,
      headers: { authorization: 'Bearer scoped-token' },
      headersPath: '/npm/',
    })

    assert.deepEqual(authorizations, [undefined])
  })

  test('drops scoped credentials when a tarball redirect leaves the configured path', async () => {
    authorizations = []
    await downloadTarball(meta(`${origin}/npm/redirect-out.tgz`), path.join(dir, 'redirect-out.tgz'), {
      registry: `${origin}/npm/`,
      headers: { authorization: 'Bearer scoped-token' },
      headersPath: '/npm/',
    })

    assert.deepEqual(authorizations, ['Bearer scoped-token', undefined])
  })

  test('keeps scoped credentials when a tarball redirect stays in the configured path', async () => {
    authorizations = []
    await downloadTarball(meta(`${origin}/npm/redirect-in.tgz`), path.join(dir, 'redirect-in.tgz'), {
      registry: `${origin}/npm/`,
      headers: { authorization: 'Bearer scoped-token' },
      headersPath: '/npm/',
    })

    assert.deepEqual(authorizations, ['Bearer scoped-token', 'Bearer scoped-token'])
  })

  test('drops scoped credentials when a metadata redirect leaves the configured path', async () => {
    authorizations = []
    await fetchVersionMeta(
      `${origin}/npm/`,
      'meta-redirect',
      '1.0.0',
      { authorization: 'Bearer scoped-token' },
      '/npm/'
    )

    assert.deepEqual(authorizations, ['Bearer scoped-token', undefined])
  })

  function meta (tarball: string): VersionMeta {
    return { dist: { tarball, integrity } }
  }
})
