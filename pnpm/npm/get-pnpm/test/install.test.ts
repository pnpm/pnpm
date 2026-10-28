import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { createHash, createSign, generateKeyPairSync } from 'node:crypto'
import fs from 'node:fs'
import http from 'node:http'
import os from 'node:os'
import path from 'node:path'
import { after, before, describe, test } from 'node:test'

import { downloadPnpm, installPnpm } from '../lib/install.js'
import type { SigningKey } from '../lib/verifySignature.js'

const VERSION = '99.0.0'

// The pinned key is npm's, so these tests sign with their own and pass it in.
// Nothing served here can satisfy the real key — which is the point of pinning.
const { privateKey, publicKey } = generateKeyPairSync('ec', { namedCurve: 'prime256v1' })
const KEYS: SigningKey[] = [{
  keyid: 'SHA256:test-key',
  key: publicKey.export({ type: 'spki', format: 'der' }).toString('base64'),
  expires: null,
}]

/** One thing broken per run, to drive each refusal separately. */
type Mode = 'ok' | 'bad-tarball' | 'bad-signature' | 'unsigned' | 'root-signatures' | 'wrong-keyid' | 'no-integrity' | 'npm-tarball-url'

let tmpDir: string
let server: http.Server
let registry: string
let mode: Mode = 'ok'
let setupLog: string
let authorizations: Array<string | undefined> = []
let servedVersion = VERSION

// The fake executable the mock serves is a `#!/bin/sh` script, so the flow that
// runs it is POSIX-only. The verification it exercises is platform-independent.
describe('installPnpm', { skip: process.platform === 'win32' }, () => {
  before(async () => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'get-pnpm-test-'))
    setupLog = path.join(tmpDir, 'setup.log')
    const tarballs: Tarballs = {
      // The wrapper carries `dist/`; the platform package carries the executable.
      pnpm: packTarball('wrapper', { 'dist/worker.js': 'export {}\n' }),
      platform: packTarball('platform', {
        pnpm: `#!/bin/sh\necho "$@" > "${setupLog}"\nls "$(dirname "$0")/dist" >> "${setupLog}"\n`,
      }),
    }

    server = http.createServer((req, res) => {
      handleRequest(req, res, tarballs)
    })
    await new Promise<void>((resolve) => {
      server.listen(0, '127.0.0.1', resolve)
    })
    registry = `http://127.0.0.1:${(server.address() as { port: number }).port}/`
  })

  after(async () => {
    await new Promise<void>((resolve) => {
      server.close(() => {
        resolve()
      })
    })
    fs.rmSync(tmpDir, { recursive: true, force: true })
  })

  const install = async (): Promise<number> => installPnpm({ versionSpec: 'latest', registry, keys: KEYS })

  test('runs setup on an executable placed next to the dist it ships with', async () => {
    mode = 'ok'
    assert.equal(await install(), 0)
    assert.equal(fs.readFileSync(setupLog, 'utf8'), 'pm setup --force\nworker.js\n')
  })

  test('passes --no-modify-path to setup', async () => {
    mode = 'ok'
    assert.equal(await installPnpm({ versionSpec: 'latest', registry, keys: KEYS, modifyPath: false }), 0)
    assert.equal(fs.readFileSync(setupLog, 'utf8'), 'pm setup --force --no-modify-path\nworker.js\n')
  })

  test('refuses --no-modify-path before running a setup older than 12.16.0', async () => {
    mode = 'ok'
    servedVersion = '12.15.0'
    fs.rmSync(setupLog, { force: true })
    try {
      await assert.rejects(
        installPnpm({ versionSpec: 'latest', registry, keys: KEYS, modifyPath: false }),
        (err: Error) => err.message.includes('pnpm 12.15.0 cannot be installed without adding it to PATH')
      )
    } finally {
      servedVersion = VERSION
    }
    assert.ok(!fs.existsSync(setupLog), 'setup is not run')
  })

  test('downloadPnpm places the executable without running setup', async () => {
    mode = 'ok'
    const dest = path.join(tmpDir, 'dest')
    fs.rmSync(setupLog, { force: true })
    const { version, binPath } = await downloadPnpm({ versionSpec: 'latest', registry, dest, keys: KEYS })

    assert.equal(version, VERSION)
    assert.equal(binPath, path.join(dest, 'pnpm'))
    assert.ok(fs.existsSync(path.join(dest, 'dist', 'worker.js')), 'dist/ is placed beside the executable')
    assert.ok(!fs.existsSync(setupLog), 'setup is not run')
    // Nothing but the installation: an unpack directory left here would be
    // installed too by a caller that hands the directory to `pnpm setup`. No
    // manifest either — the fixture is a v12-shaped release, whose `dist/` is
    // self-contained.
    assert.deepEqual(fs.readdirSync(dest).sort(), ['dist', 'pnpm'])
  })

  test('sends credentials to every registry request', async () => {
    mode = 'ok'
    authorizations = []
    const dest = path.join(tmpDir, 'authenticated')

    await downloadPnpm({
      versionSpec: 'latest',
      registry,
      dest,
      headers: { authorization: 'Bearer a-token' },
      keys: KEYS,
    })

    assert.equal(authorizations.length, 5)
    assert.deepEqual(new Set(authorizations), new Set(['Bearer a-token']))
  })

  test('downloads from the registry that served the metadata, not from npm', async () => {
    mode = 'npm-tarball-url'
    const dest = path.join(tmpDir, 'rehosted')

    const { binPath } = await downloadPnpm({ versionSpec: 'latest', registry, dest, keys: KEYS })

    assert.equal(fs.readFileSync(binPath, 'utf8').startsWith('#!/bin/sh'), true)
  })

  test('leaves no temporary directory behind', async () => {
    mode = 'ok'
    const before = leftoverTmpDirs()
    await install()
    assert.deepEqual(leftoverTmpDirs(), before)
  })

  test('refuses a tarball that does not match its checksum', async () => {
    mode = 'bad-tarball'
    await assert.rejects(install, /does not match the checksum/)
  })

  test('refuses a checksum the signature does not cover', async () => {
    mode = 'bad-signature'
    await assert.rejects(install, /is not valid/)
  })

  test('refuses an unsigned package', async () => {
    mode = 'unsigned'
    await assert.rejects(install, /carries no npm registry signature/)
  })

  test('takes the signatures from the package document when the version document has none', async () => {
    mode = 'root-signatures'
    assert.equal(await install(), 0)
  })

  test('refuses a package signed with a key that is not pinned', async () => {
    mode = 'wrong-keyid'
    await assert.rejects(install, /unexpected npm key/)
  })

  test('refuses a package the registry published no checksum for', async () => {
    mode = 'no-integrity'
    await assert.rejects(install, /published no checksum/)
  })
})

interface Tarballs {
  pnpm: string
  platform: string
}

function handleRequest (req: http.IncomingMessage, res: http.ServerResponse, tarballs: Tarballs): void {
  authorizations.push(req.headers.authorization)
  const url = decodeURIComponent(req.url!)
  const json = (body: unknown): void => {
    res.writeHead(200, { 'content-type': 'application/json' })
    res.end(JSON.stringify(body))
  }

  if (url.endsWith(`/${servedVersion}`)) {
    const kind = url === `/pnpm/${servedVersion}` ? 'pnpm' : 'platform'
    const name = url.slice(1, url.lastIndexOf('/'))
    json(versionMetaFor(name, kind, tarballs))
    return
  }
  if (url === '/pnpm.tgz' || url === '/platform.tgz') {
    const bytes = fs.readFileSync(tarballs[url === '/pnpm.tgz' ? 'pnpm' : 'platform'])
    res.writeHead(200, { 'content-type': 'application/octet-stream' })
    res.end(mode === 'bad-tarball' ? Buffer.concat([bytes, Buffer.from('tampered')]) : bytes)
    return
  }
  if (url.endsWith('.tgz')) {
    res.writeHead(404)
    res.end()
    return
  }
  json(packumentFor(url.slice(1), tarballs))
}

function packumentFor (name: string, tarballs: Tarballs): unknown {
  const kind = name === 'pnpm' ? 'pnpm' : 'platform'
  return {
    'dist-tags': { latest: servedVersion },
    versions: {
      [servedVersion]: mode === 'root-signatures' ? { dist: { signatures: signaturesFor(name, kind, tarballs) } } : {},
    },
  }
}

function versionMetaFor (name: string, kind: keyof Tarballs, tarballs: Tarballs): unknown {
  const integrity = integrityOf(kind, tarballs)
  return {
    dist: {
      // An npm URL is what a registry proxying npm hands back, and has
      // to be re-hosted onto the registry that answered.
      tarball: mode === 'npm-tarball-url'
        ? `https://registry.npmjs.org/${kind}.tgz`
        : `${registry}${kind}.tgz`,
      ...(mode === 'no-integrity' ? {} : { integrity }),
      // A proxy that strips the signatures here leaves them in the package document.
      ...(mode === 'root-signatures' ? {} : { signatures: signaturesFor(name, kind, tarballs) }),
    },
  }
}

function signaturesFor (name: string, kind: keyof Tarballs, tarballs: Tarballs): unknown[] {
  if (mode === 'unsigned') return []
  // Signing a different checksum than the one served is what a tampered
  // checksum looks like: the bytes and the metadata agree, the signature
  // does not cover them.
  const signed = mode === 'bad-signature' ? `sha512-${'A'.repeat(86)}==` : integrityOf(kind, tarballs)
  return [{
    keyid: mode === 'wrong-keyid' ? 'SHA256:not-pinned' : KEYS[0]!.keyid,
    sig: createSign('SHA256').update(`${name}@${servedVersion}:${signed}`).sign(privateKey, 'base64'),
  }]
}

function integrityOf (kind: keyof Tarballs, tarballs: Tarballs): string {
  return `sha512-${createHash('sha512').update(fs.readFileSync(tarballs[kind])).digest('base64')}`
}

function leftoverTmpDirs (): string[] {
  return fs.readdirSync(os.tmpdir()).filter((entry) => entry.startsWith('pnpm-install-'))
}

function packTarball (name: string, files: Record<string, string>): string {
  const contentDir = path.join(tmpDir, name, 'package')
  for (const [filePath, content] of Object.entries(files)) {
    const dest = path.join(contentDir, filePath)
    fs.mkdirSync(path.dirname(dest), { recursive: true })
    fs.writeFileSync(dest, content, { mode: 0o755 })
  }
  const tarball = path.join(tmpDir, `${name}.tgz`)
  const { status, stderr } = spawnSync('tar', ['-czf', tarball, '-C', path.join(tmpDir, name), 'package'], { encoding: 'utf8' })
  if (status !== 0) throw new Error(`could not create the ${name} fixture tarball: ${stderr}`)
  return tarball
}
