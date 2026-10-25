import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { createHash, createSign, generateKeyPairSync } from 'node:crypto'
import fs from 'node:fs'
import http from 'node:http'
import net from 'node:net'
import os from 'node:os'
import path from 'node:path'
import type { Duplex } from 'node:stream'
import { after, before, beforeEach, describe, test } from 'node:test'
import zlib from 'node:zlib'

import { downloadPnpmExecutable } from '../lib/index.js'
import { platformPackageName } from '../lib/platformPackageName.js'
import type { SigningKey } from '../lib/verifySignature.js'

const VERSION = '99.0.0'
const EXECUTABLE = process.platform === 'win32' ? 'pnpm.exe' : 'pnpm'
const CONTENT = 'the pnpm executable, supposedly'

// The pinned key is npm's, so these tests sign with their own and pass it in.
const { privateKey, publicKey } = generateKeyPairSync('ec', { namedCurve: 'prime256v1' })
const KEYS: SigningKey[] = [{
  keyid: 'SHA256:test-key',
  key: publicKey.export({ type: 'spki', format: 'der' }).toString('base64'),
  expires: null,
}]

/** One thing broken per run, to drive each refusal separately. */
type Mode = 'ok' | 'bad-tarball' | 'bad-signature' | 'unsigned' | 'root-signatures' | 'root-bad-signature' | 'no-integrity' | 'no-executable' | 'npm-tarball-url' | 'truncated'

let tmpDir: string
let server: http.Server
let cdn: http.Server
let proxy: http.Server
/** An origin nothing downloads from, to probe the route a request takes. */
let probe: http.Server
let registry: string
let mode: Mode = 'ok'
let requests: Array<{ path: string, authorization?: string }> = []
let cdnRequests: Array<{ path: string, authorization?: string }> = []
/** `host:port` of every origin a client reached through the proxy. */
let proxiedOrigins: string[] = []
// The servers these tests start, the only hosts the proxy forwards to.
const fixtureHosts: string[] = []
/** When set, the second tarball request of a test waits on it before being served. */
let holdSecondTarball: Promise<void> | null = null
/** Registry requests left to answer by destroying the connection instead of responding. */
let registryResetsRemaining = 0
/** Tarball requests left to answer by destroying the connection instead of responding. */
let tarballResetsRemaining = 0
/** Tarball requests left to answer by sending half the body and then destroying the connection. */
let tarballCutsRemaining = 0
/** Statuses to answer the next registry requests with, one per request, instead of serving them. */
let registryStatuses: number[] = []
/** Registry requests left to answer with a 200 that has no body. */
let registryEmptiesRemaining = 0
/** Tarball requests left to answer with a 200 that has no body. */
let tarballEmptiesRemaining = 0

// Node applies a proxy after startup from 24.14; before that the request goes
// direct, which the proxy tests have nothing to say about.
const PROXY_UNSUPPORTED = typeof (http as { setGlobalProxyFromEnv?: unknown }).setGlobalProxyFromEnv !== 'function'

describe('downloadPnpmExecutable', () => {
  before(async () => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'get-pnpm-exe-test-'))
    tarballs = {
      ok: packTarball('ok', { [EXECUTABLE]: CONTENT }),
      empty: packTarball('empty', { 'README.md': 'nothing to run here\n' }),
      // Big enough to span several blocks, so cutting the tail leaves the
      // executable's declared size unmet rather than merely losing padding.
      truncated: truncate(packTarball('big', { [EXECUTABLE]: 'x'.repeat(5000) })),
    }

    cdn = http.createServer(handleCdnRequest)
    await listen(cdn)

    probe = http.createServer((req, res) => {
      res.end()
    })
    await listen(probe)

    server = http.createServer(handleRegistryRequest)
    await listen(server)
    registry = `${addressOf(server)}/`

    // A forwarding proxy. Which of the two forms a proxied client uses for a
    // plain-http origin depends on the Node release: a CONNECT tunnel, or a
    // request for the full URL that the proxy relays. Both are served, and both
    // record the origin reached. The relay gets an agent of its own, since the
    // global one is what the code under test points at the proxy, and the relay
    // would otherwise loop back through it to itself.
    proxy = http.createServer(relayProxyRequest)
    proxy.on('connect', tunnelProxyConnection)
    await listen(proxy)
  })

  after(async () => {
    await Promise.all([close(server), close(cdn), close(proxy), close(probe)])
    fs.rmSync(tmpDir, { recursive: true, force: true })
  })

  beforeEach(() => {
    mode = 'ok'
    requests = []
    cdnRequests = []
    proxiedOrigins = []
    holdSecondTarball = null
    registryResetsRemaining = 0
    tarballResetsRemaining = 0
    tarballCutsRemaining = 0
    registryStatuses = []
    registryEmptiesRemaining = 0
    tarballEmptiesRemaining = 0
  })

  test('places the executable and nothing else', async () => {
    const destPath = destIn('placed')

    const { packageName } = await downloadPnpmExecutable({ version: VERSION, registry, destPath, keys: KEYS })

    assert.equal(packageName, packageNameFor())
    assert.equal(fs.readFileSync(destPath, 'utf8'), CONTENT)
    assert.deepEqual(fs.readdirSync(path.dirname(destPath)), [path.basename(destPath)])
  })

  test('marks the executable executable', { skip: process.platform === 'win32' }, async () => {
    const destPath = destIn('mode')

    await downloadPnpmExecutable({ version: VERSION, registry, destPath, keys: KEYS })

    assert.equal(fs.statSync(destPath).mode & 0o111, 0o111)
  })

  test('asks for the version it was given, without resolving dist-tags', async () => {
    await downloadPnpmExecutable({ version: VERSION, registry, destPath: destIn('exact'), keys: KEYS })

    assert.deepEqual(requests.map(({ path: asked }) => asked), [`/${packageNameFor()}/${VERSION}`])
  })

  test('sends credentials to the registry and not to the download host', async () => {
    const headers = { authorization: 'Bearer a-token' }

    await downloadPnpmExecutable({ version: VERSION, registry, destPath: destIn('auth'), headers, keys: KEYS })

    assert.equal(requests[0]!.authorization, 'Bearer a-token')
    assert.equal(cdnRequests[0]!.authorization, undefined)
  })

  test('routes the download through the proxy the environment names', { skip: PROXY_UNSUPPORTED }, async () => {
    const destPath = destIn('proxied')

    await withEnv({ HTTP_PROXY: addressOf(proxy), NO_PROXY: undefined }, async () => {
      await download(destPath)
    })

    assert.equal(fs.readFileSync(destPath, 'utf8'), CONTENT)
    assert.deepEqual(new Set(proxiedOrigins), new Set([hostOf(server), hostOf(cdn)]))
    assert.equal(requests.length, 1, 'the registry was reached through the proxy')
    assert.equal(cdnRequests.length, 1, 'the download host was reached through the proxy')
  })

  test('leaves the proxy alone for a host NO_PROXY excludes', { skip: PROXY_UNSUPPORTED }, async () => {
    const destPath = destIn('unproxied')

    await withEnv({ HTTP_PROXY: addressOf(proxy), NO_PROXY: '127.0.0.1' }, async () => {
      await download(destPath)
    })

    assert.equal(fs.readFileSync(destPath, 'utf8'), CONTENT)
    assert.deepEqual(proxiedOrigins, [])
  })

  test('stops using the proxy once the download is done', { skip: PROXY_UNSUPPORTED }, async () => {
    await withEnv({ HTTP_PROXY: addressOf(proxy), NO_PROXY: undefined }, async () => {
      await download(destIn('restored'))
      proxiedOrigins = []
      await fetch(`${registry}${packageNameFor()}/${VERSION}`)
    })

    assert.deepEqual(proxiedOrigins, [], 'a request after the download went direct')
  })

  test('keeps the proxy until the last of two overlapping downloads is done', { skip: PROXY_UNSUPPORTED }, async () => {
    // One download is held at its tarball until the other has finished. A
    // request made in that window still goes through the proxy; the direct
    // route comes back only once the held download is done too. The probe is an
    // origin nothing has reached yet, so no pooled tunnel can hide the route.
    let release!: () => void
    holdSecondTarball = new Promise((resolve) => {
      release = resolve
    })
    await withEnv({ HTTP_PROXY: addressOf(proxy), NO_PROXY: undefined }, async () => {
      const downloads = [download(destIn('overlap-a')), download(destIn('overlap-b'))]
      await Promise.race(downloads)
      proxiedOrigins = []

      await fetch(`${addressOf(probe)}/held`)

      assert.deepEqual(proxiedOrigins, [hostOf(probe)], 'a request while one download is still running went through the proxy')
      release()
      await Promise.all(downloads)
      proxiedOrigins = []
      await fetch(`${addressOf(probe)}/released`)
    })

    assert.deepEqual(proxiedOrigins, [], 'a request after both downloads went direct')
  })

  test('activates the proxy on a retry after a malformed proxy URL was rejected', { skip: PROXY_UNSUPPORTED }, async () => {
    await withEnv({ HTTP_PROXY: 'not a url', NO_PROXY: undefined }, async () => {
      await assert.rejects(download(destIn('malformed')))
    })
    const destPath = destIn('retried')

    await withEnv({ HTTP_PROXY: addressOf(proxy), NO_PROXY: undefined }, async () => {
      await download(destPath)
    })

    assert.equal(fs.readFileSync(destPath, 'utf8'), CONTENT)
    assert.deepEqual(new Set(proxiedOrigins), new Set([hostOf(server), hostOf(cdn)]), 'the retry went through the proxy')
  })

  test('retries a registry request that had its connection reset', async () => {
    registryResetsRemaining = 1
    const destPath = destIn('registry-reset')

    await downloadPnpmExecutable({ version: VERSION, registry, destPath, keys: KEYS })

    assert.equal(fs.readFileSync(destPath, 'utf8'), CONTENT)
    assert.equal(requests.length, 1, 'only the successful attempt reached the handler that records requests')
  })

  test('retries a tarball download that had its connection reset', async () => {
    tarballResetsRemaining = 1
    const destPath = destIn('tarball-reset')

    await downloadPnpmExecutable({ version: VERSION, registry, destPath, keys: KEYS })

    assert.equal(fs.readFileSync(destPath, 'utf8'), CONTENT)
    assert.equal(cdnRequests.length, 1, 'only the successful attempt reached the handler that records requests')
  })

  test('gives up on a registry connection that keeps resetting', async () => {
    registryResetsRemaining = 5
    const destPath = destIn('registry-exhausted')

    await assert.rejects(
      downloadPnpmExecutable({ version: VERSION, registry, destPath, keys: KEYS }),
      /Could not reach/
    )
    assert.deepEqual(fs.readdirSync(path.dirname(destPath)), [])
  })

  test('retries a tarball download whose connection was lost partway through the body', async () => {
    tarballCutsRemaining = 1
    const destPath = destIn('tarball-cut')

    await downloadPnpmExecutable({ version: VERSION, registry, destPath, keys: KEYS })

    assert.equal(fs.readFileSync(destPath, 'utf8'), CONTENT)
    assert.equal(cdnRequests.length, 2)
  })

  test('retries a registry request answered with a 5xx or a 429', async () => {
    registryStatuses = [503, 429]
    const destPath = destIn('registry-5xx')

    await downloadPnpmExecutable({ version: VERSION, registry, destPath, keys: KEYS })

    assert.equal(fs.readFileSync(destPath, 'utf8'), CONTENT)
    assert.equal(requests.length, 3)
  })

  test('retries a registry response that came back empty', async () => {
    registryEmptiesRemaining = 1
    const destPath = destIn('registry-empty')

    await downloadPnpmExecutable({ version: VERSION, registry, destPath, keys: KEYS })

    assert.equal(fs.readFileSync(destPath, 'utf8'), CONTENT)
    assert.equal(requests.length, 2)
  })

  test('retries a tarball download that came back empty', async () => {
    tarballEmptiesRemaining = 1
    const destPath = destIn('tarball-empty')

    await downloadPnpmExecutable({ version: VERSION, registry, destPath, keys: KEYS })

    assert.equal(fs.readFileSync(destPath, 'utf8'), CONTENT)
    assert.equal(cdnRequests.length, 2)
  })

  test('does not retry a registry request answered with a 404', async () => {
    registryStatuses = [404]
    const destPath = destIn('registry-404')

    await assert.rejects(
      downloadPnpmExecutable({ version: VERSION, registry, destPath, keys: KEYS }),
      /404/
    )
    assert.equal(requests.length, 1)
  })

  test('re-hosts an npm tarball URL onto the registry that served the metadata', async () => {
    mode = 'npm-tarball-url'
    const destPath = destIn('rehosted')

    await downloadPnpmExecutable({ version: VERSION, registry, destPath, keys: KEYS })

    assert.equal(fs.readFileSync(destPath, 'utf8'), CONTENT)
    assert.equal(cdnRequests.length, 0, 'the download stayed on the registry')
  })

  test('keeps a registry subpath that does not end in a slash', async () => {
    const destPath = destIn('subpath')

    await downloadPnpmExecutable({ version: VERSION, registry: `${addressOf(server)}/npm`, destPath, keys: KEYS })

    assert.equal(fs.readFileSync(destPath, 'utf8'), CONTENT)
    assert.deepEqual(requests.map(({ path: asked }) => asked), [`/npm/${packageNameFor()}/${VERSION}`])
  })

  test('refuses an archive that ends inside the executable', async () => {
    mode = 'truncated'
    const destPath = destIn('truncated')

    await assert.rejects(download(destPath), /ends after \d+ of the 5000 bytes/)
    assert.deepEqual(fs.readdirSync(path.dirname(destPath)), [])
  })

  test('accepts an executable a concurrent call already placed', async () => {
    const destPath = destIn('raced')
    fs.writeFileSync(destPath, CONTENT)

    await downloadPnpmExecutable({ version: VERSION, registry, destPath, keys: KEYS })

    assert.equal(fs.readFileSync(destPath, 'utf8'), CONTENT)
    assert.deepEqual(fs.readdirSync(path.dirname(destPath)), [path.basename(destPath)])
  })

  // The errno differs per platform — EISDIR on Linux, EPERM on Windows — so
  // what is asserted is that the directory survives rather than how it says so.
  test('refuses a destination that is not a file', async () => {
    const destPath = destIn('directory')
    fs.mkdirSync(destPath)

    await assert.rejects(download(destPath), /rename/)
    assert.ok(fs.statSync(destPath).isDirectory(), 'the directory is left alone')
    assert.deepEqual(fs.readdirSync(path.dirname(destPath)), [path.basename(destPath)])
  })

  test('refuses a tarball that does not match its checksum', async () => {
    mode = 'bad-tarball'
    const destPath = destIn('tampered')

    await assert.rejects(download(destPath), /does not match the checksum/)
    assert.deepEqual(fs.readdirSync(path.dirname(destPath)), [])
  })

  test('refuses a checksum the signature does not cover', async () => {
    mode = 'bad-signature'
    const destPath = destIn('unsigned-checksum')

    await assert.rejects(download(destPath), /is not valid/)
    assert.deepEqual(fs.readdirSync(path.dirname(destPath)), [])
  })

  test('refuses an unsigned package', async () => {
    mode = 'unsigned'

    await assert.rejects(download(destIn('unsigned')), /carries no npm registry signature/)
  })

  test('takes the signatures from the package document when the version document has none', async () => {
    mode = 'root-signatures'
    const destPath = destIn('root-signatures')

    await download(destPath)

    assert.equal(fs.readFileSync(destPath, 'utf8'), CONTENT)
    assert.ok(requests.some(({ path }) => path === `/${packageNameFor()}`), 'the package document is requested')
  })

  test('refuses package-document signatures that do not cover the checksum served', async () => {
    mode = 'root-bad-signature'

    await assert.rejects(download(destIn('root-bad-signature')), /is not valid/)
  })

  test('does not request the package document when the version document is signed', async () => {
    await download(destIn('version-signed'))

    assert.ok(!requests.some(({ path }) => path === `/${packageNameFor()}`))
  })

  test('takes an unsigned package when the caller waives the signature', async () => {
    mode = 'unsigned'
    const destPath = destIn('signature-waived')

    await downloadPnpmExecutable({ version: VERSION, registry, destPath, verifySignature: false })

    assert.equal(fs.readFileSync(destPath, 'utf8'), CONTENT)
  })

  test('still checks the checksum when the caller waives the signature', async () => {
    mode = 'bad-tarball'
    const destPath = destIn('waived-but-tampered')

    await assert.rejects(
      downloadPnpmExecutable({ version: VERSION, registry, destPath, verifySignature: false }),
      /does not match the checksum/
    )
  })

  test('refuses a package the registry published no checksum for', async () => {
    mode = 'no-integrity'

    await assert.rejects(download(destIn('no-integrity')), /published no checksum/)
  })

  test('reports a tarball that carries no executable', async () => {
    mode = 'no-executable'
    const destPath = destIn('no-executable')

    await assert.rejects(download(destPath), /contains no package\//)
    assert.deepEqual(fs.readdirSync(path.dirname(destPath)), [])
  })
})

/** Runs `fn` with the proxy variables set as given, then puts them back. */
let tarballs: { ok: string, empty: string, truncated: string }

function bytesFor (mode: Mode): Buffer {
  if (mode === 'no-executable') return fs.readFileSync(tarballs.empty)
  if (mode === 'truncated') return fs.readFileSync(tarballs.truncated)
  return fs.readFileSync(tarballs.ok)
}

function serveTarball (res: http.ServerResponse): void {
  const bytes = bytesFor(mode)
  res.writeHead(200, { 'content-type': 'application/octet-stream' })
  res.end(mode === 'bad-tarball' ? Buffer.concat([bytes, Buffer.from('tampered')]) : bytes)
}

async function handleCdnRequest (req: http.IncomingMessage, res: http.ServerResponse): Promise<void> {
  if (tarballResetsRemaining > 0) {
    tarballResetsRemaining--
    req.socket.destroy()
    return
  }
  cdnRequests.push({ path: req.url!, authorization: req.headers.authorization })
  if (tarballEmptiesRemaining > 0) {
    tarballEmptiesRemaining--
    res.writeHead(200, { 'content-type': 'application/octet-stream' })
    res.end()
    return
  }
  if (tarballCutsRemaining > 0) {
    tarballCutsRemaining--
    const bytes = bytesFor(mode)
    res.writeHead(200, { 'content-type': 'application/octet-stream', 'content-length': bytes.length })
    res.write(bytes.subarray(0, bytes.length >> 1), () => {
      req.socket.destroy()
    })
    return
  }
  if (cdnRequests.length === 2 && holdSecondTarball != null) await holdSecondTarball
  serveTarball(res)
}

function handleRegistryRequest (req: http.IncomingMessage, res: http.ServerResponse): void {
  if (registryResetsRemaining > 0) {
    registryResetsRemaining--
    req.socket.destroy()
    return
  }
  const url = decodeURIComponent(req.url!)
  requests.push({ path: url, authorization: req.headers.authorization })
  if (registryEmptiesRemaining > 0) {
    registryEmptiesRemaining--
    res.writeHead(200, { 'content-type': 'application/json' })
    res.end()
    return
  }
  const status = registryStatuses.shift()
  if (status != null) {
    res.writeHead(status)
    res.end('not now')
    return
  }
  if (url.endsWith(`/${VERSION}`)) {
    // The registry may be served from a subpath, which is not part of the
    // package name the signature covers.
    const name = url.slice(1, url.lastIndexOf('/')).replace(/^npm\//, '')
    res.writeHead(200, { 'content-type': 'application/json' })
    res.end(JSON.stringify(versionMetaFor(name)))
    return
  }
  if (!url.includes('/-/')) {
    const name = url.slice(1).replace(/^npm\//, '')
    res.writeHead(200, { 'content-type': 'application/json' })
    res.end(JSON.stringify(packumentFor(name)))
    return
  }
  serveTarball(res)
}

function versionMetaFor (name: string): unknown {
  const integrity = integrityOf(bytesFor(mode))
  return {
    dist: {
      // An npm URL is what a registry proxying npm hands back, and has to
      // be re-hosted onto the registry that answered.
      tarball: mode === 'npm-tarball-url'
        ? `https://registry.npmjs.org/${name}/-/tarball.tgz`
        : `${addressOf(cdn)}/${name}/-/tarball.tgz`,
      ...(mode === 'no-integrity' ? {} : { integrity }),
      // A proxy that strips the signatures here leaves them in the package document.
      ...(mode === 'root-signatures' || mode === 'root-bad-signature' ? {} : { signatures: signaturesFor(name) }),
    },
  }
}

function packumentFor (name: string): unknown {
  const signed = mode === 'root-signatures' || mode === 'root-bad-signature'
  return {
    'dist-tags': { latest: VERSION },
    versions: { [VERSION]: { dist: signed ? { signatures: signaturesFor(name) } : {} } },
  }
}

function signaturesFor (name: string): unknown[] {
  if (mode === 'unsigned') return []
  // Signing a checksum other than the one served is what a tampered checksum
  // looks like: bytes and metadata agree, the signature does not cover them.
  const signed = mode === 'bad-signature' || mode === 'root-bad-signature'
    ? `sha512-${'A'.repeat(86)}==`
    : integrityOf(bytesFor(mode))
  return [{
    keyid: KEYS[0]!.keyid,
    sig: createSign('SHA256').update(`${name}@${VERSION}:${signed}`).sign(privateKey, 'base64'),
  }]
}

function integrityOf (bytes: Buffer): string {
  return `sha512-${createHash('sha512').update(bytes).digest('base64')}`
}

function relayProxyRequest (req: http.IncomingMessage, res: http.ServerResponse): void {
  const requested = new URL(req.url!)
  proxiedOrigins.push(requested.host)
  const host = fixtureHosts.find((fixtureHost) => fixtureHost === requested.host)
  if (host == null) {
    res.writeHead(403)
    res.end()
    return
  }
  const target = new URL(`${requested.pathname}${requested.search}`, `http://${host}`)
  const relay = http.request(target, { method: req.method, headers: req.headers, agent: new http.Agent() }, (upstream) => {
    res.writeHead(upstream.statusCode!, upstream.headers)
    upstream.pipe(res)
  })
  req.pipe(relay)
}

function tunnelProxyConnection (req: http.IncomingMessage, socket: Duplex, head: Buffer): void {
  proxiedOrigins.push(req.url!)
  const host = fixtureHosts.find((fixtureHost) => fixtureHost === req.url)
  if (host == null) {
    socket.end('HTTP/1.1 403 Forbidden\r\n\r\n')
    return
  }
  const { hostname, port } = new URL(`http://${host}`)
  const upstream = net.connect(Number(port), hostname, () => {
    socket.write('HTTP/1.1 200 Connection Established\r\n\r\n')
    upstream.write(head)
    upstream.pipe(socket)
    socket.pipe(upstream)
  })
  upstream.on('error', () => {
    socket.destroy()
  })
  socket.on('error', () => {
    upstream.destroy()
  })
}

async function withEnv (vars: Record<string, string | undefined>, fn: () => Promise<void>): Promise<void> {
  const names = ['HTTP_PROXY', 'HTTPS_PROXY', 'NO_PROXY', 'http_proxy', 'https_proxy', 'no_proxy']
  const saved = Object.fromEntries(names.map((name) => [name, process.env[name]]))
  for (const name of names) delete process.env[name]
  for (const [name, value] of Object.entries(vars)) {
    if (value !== undefined) process.env[name] = value
  }
  try {
    await fn()
  } finally {
    for (const name of names) {
      if (saved[name] === undefined) delete process.env[name]
      else process.env[name] = saved[name]
    }
  }
}

async function download (destPath: string): Promise<unknown> {
  return downloadPnpmExecutable({ version: VERSION, registry, destPath, keys: KEYS })
}

function packageNameFor (): string {
  return platformPackageName({ major: 99, platform: process.platform, arch: process.arch, musl: false })
}

/** A destination of its own per test, so leftovers are visible. */
function destIn (name: string): string {
  const dir = path.join(tmpDir, name)
  fs.mkdirSync(dir, { recursive: true })
  return path.join(dir, EXECUTABLE)
}

/**
 * Cuts an archive off one block into the executable, so the entry declares more
 * than the archive holds. Located rather than measured from the end, because
 * tar pads an archive out to a record far longer than its content.
 */
function truncate (tarball: string): string {
  const unpacked = zlib.gunzipSync(fs.readFileSync(tarball))
  for (let offset = 0; offset + 512 <= unpacked.length; offset += 512) {
    const name = unpacked.toString('utf8', offset, offset + 100).replace(/\0.*$/s, '')
    if (name !== `package/${EXECUTABLE}`) continue
    const cut = path.join(tmpDir, 'truncated.tgz')
    fs.writeFileSync(cut, zlib.gzipSync(unpacked.subarray(0, offset + 512 + 512)))
    return cut
  }
  throw new Error(`no ${EXECUTABLE} entry to truncate in ${tarball}`)
}

function packTarball (name: string, files: Record<string, string>): string {
  const contentDir = path.join(tmpDir, name, 'package')
  for (const [filePath, content] of Object.entries(files)) {
    const dest = path.join(contentDir, filePath)
    fs.mkdirSync(path.dirname(dest), { recursive: true })
    fs.writeFileSync(dest, content)
  }
  const tarball = path.join(tmpDir, `${name}.tgz`)
  const { status, stderr } = spawnSync('tar', ['-czf', tarball, '-C', path.join(tmpDir, name), 'package'], { encoding: 'utf8' })
  if (status !== 0) throw new Error(`could not create the ${name} fixture tarball: ${stderr}`)
  return tarball
}

async function listen (target: http.Server): Promise<void> {
  await new Promise<void>((resolve) => {
    target.listen(0, '127.0.0.1', resolve)
  })
  fixtureHosts.push(hostOf(target))
}

async function close (target: http.Server): Promise<void> {
  await new Promise<void>((resolve) => {
    target.close(() => {
      resolve()
    })
  })
}

function addressOf (target: http.Server): string {
  return `http://${hostOf(target)}`
}

function hostOf (target: http.Server): string {
  return `127.0.0.1:${(target.address() as { port: number }).port}`
}
