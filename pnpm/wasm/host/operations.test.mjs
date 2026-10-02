import assert from 'node:assert/strict'
import { once } from 'node:events'
import { createServer } from 'node:http'
import { gzipSync } from 'node:zlib'
import { closeSync, mkdtempSync, openSync, readFileSync, rmSync, writeFileSync, writeSync } from 'node:fs'
import { constants, tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'

import { createHostServices } from './operations.mjs'

test('HTTP returns headers before the complete body, with bounded binary reads', async t => {
  const host = createHostServices()
  t.after(() => host.close())
  let finish
  const url = await serve(t, (request, response) => {
    assert.equal(request.headers.authorization, 'Bearer secret')
    response.writeHead(200, { 'x-pnpm-test': 'streamed' })
    response.write(Buffer.from([0, 255, 127]))
    finish = () => response.end(Buffer.alloc(70000, 42))
  })
  const response = await host.dispatch({ operation: 'network.request', url, headers: { authorization: 'Bearer secret' } })
  assert.equal(response.status, 200)
  assert.ok(response.headers.some(([name, value]) => name === 'x-pnpm-test' && value === 'streamed'))
  assert.deepEqual(await host.dispatch({ operation: 'stream.read', handle: response.handle, maxBytes: 2 }), {
    bytes: Uint8Array.from([0, 255]), done: false,
  })
  finish()
  const body = await readAll(host, response.handle, 1024)
  assert.equal(body.length, 70001)
  assert.equal(body[0], 127)
  assert.ok(body.subarray(1).every(byte => byte === 42))
})

test('HTTP redirects are returned without forwarding credentials', async t => {
  const host = createHostServices()
  t.after(() => host.close())
  let redirectedRequests = 0
  const target = await serve(t, (request, response) => {
    redirectedRequests++
    response.end('unexpected')
  })
  const url = await serve(t, (request, response) => {
    response.writeHead(302, { location: target })
    response.end()
  })
  const response = await host.dispatch({ operation: 'network.request', url, headers: { authorization: 'Bearer secret' } })
  assert.equal(response.status, 302)
  assert.equal(redirectedRequests, 0)
})

test('HTTP body inactivity and host shutdown cancel outstanding requests', async t => {
  const host = createHostServices()
  t.after(() => host.close())
  const url = await serve(t, (request, response) => {
    response.writeHead(200)
    response.flushHeaders()
  })
  const response = await host.dispatch({ operation: 'network.request', url, timeoutMs: 100 })
  await assert.rejects(host.dispatch({ operation: 'stream.read', handle: response.handle }), /timed out/)
  const pendingUrl = await serve(t, () => {})
  const pending = host.dispatch({ operation: 'network.request', url: pendingUrl })
  const rejected = assert.rejects(pending, /abort/i)
  await host.close()
  await rejected
})

test('process preserves arguments, environment, working directory, binary pipes and exit status', async t => {
  const host = createHostServices()
  t.after(() => host.close())
  const script = 'process.stdin.pipe(process.stdout); process.stderr.write(JSON.stringify([process.argv[1], process.env.PNPM_WASM_TEST, process.cwd()])); process.exitCode = 17'
  const child = await host.dispatch({
    operation: 'process.spawn', program: process.execPath, args: ['-e', script, 'a b; $HOME'],
    env: { PNPM_WASM_TEST: 'exact' }, cwd: process.cwd(),
  })
  await host.dispatch({ operation: 'process.write', handle: child.handle, bytes: Uint8Array.from([0, 255, 13]) })
  await host.dispatch({ operation: 'process.end', handle: child.handle })
  const [stdout, stderr, exit] = await Promise.all([
    readAll(host, child.stdout), readAll(host, child.stderr),
    host.dispatch({ operation: 'process.wait', handle: child.handle }),
  ])
  assert.deepEqual(stdout, Buffer.from([0, 255, 13]))
  assert.deepEqual(JSON.parse(stderr), ['a b; $HOME', 'exact', process.cwd()])
  assert.deepEqual(exit, { code: 17, signal: null, signalNumber: null })
})

test('process spawn failures propagate and shutdown kills pending children', async t => {
  const host = createHostServices()
  t.after(() => host.close())
  await assert.rejects(host.dispatch({ operation: 'process.spawn', program: '/pnpm-wasm-missing-executable' }), { code: 'ENOENT' })
  const child = await host.dispatch({ operation: 'process.spawn', program: process.execPath, args: ['-e', 'setInterval(() => {}, 1000)'] })
  const completion = host.dispatch({ operation: 'process.wait', handle: child.handle })
  await host.close()
  assert.deepEqual(await completion, { code: null, signal: 'SIGKILL', signalNumber: constants.signals.SIGKILL })
})

test('invalid handles and read sizes fail without silently succeeding', async t => {
  const host = createHostServices()
  t.after(() => host.close())
  await assert.rejects(host.dispatch({ operation: 'stream.read', handle: 1 }), /Invalid stream handle/)
  const url = await serve(t, (request, response) => response.end('hello'))
  const response = await host.dispatch({ operation: 'network.request', url })
  await assert.rejects(host.dispatch({ operation: 'stream.read', handle: response.handle, maxBytes: 65537 }), /read size/)
  await host.dispatch({ operation: 'resource.close', handle: response.handle })
  await assert.rejects(host.dispatch({ operation: 'stream.read', handle: response.handle }), /Invalid stream handle/)
})

async function serve (t, handler) {
  const server = createServer(handler)
  server.listen(0, '127.0.0.1')
  await once(server, 'listening')
  t.after(() => {
    server.closeAllConnections()
    return new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve()))
  })
  return `http://127.0.0.1:${server.address().port}/`
}

async function readAll (host, handle, maxBytes = 65536) {
  const chunks = []
  while (true) {
    // eslint-disable-next-line no-await-in-loop -- Each read consumes the next chunk from the same stream.
    const { bytes, done } = await host.dispatch({ operation: 'stream.read', handle, maxBytes })
    if (done) return Buffer.concat(chunks)
    assert.ok(bytes.length <= maxBytes)
    chunks.push(bytes)
  }
}

test('per-operation cancellation aborts pending fetches and subprocesses', async t => {
  const host = createHostServices()
  t.after(() => host.close())
  const requested = Promise.withResolvers()
  const url = await serve(t, () => requested.resolve())
  const networkController = new AbortController()
  const pending = host.dispatch({ operation: 'network.request', url }, { signal: networkController.signal })
  const rejected = assert.rejects(pending, /abort/i)
  await requested.promise
  networkController.abort()
  await rejected
  const processController = new AbortController()
  const child = await host.dispatch({
    operation: 'process.spawn', program: process.execPath, args: ['-e', 'setInterval(() => {}, 1000)'],
  }, { signal: processController.signal })
  const completion = host.dispatch({ operation: 'process.wait', handle: child.handle })
  const processRejected = assert.rejects(completion, { name: 'AbortError' })
  processController.abort()
  await processRejected
})

test('HTTP total deadline also limits a body that keeps making progress', async t => {
  const host = createHostServices()
  t.after(() => host.close())
  const url = await serve(t, (request, response) => {
    response.write('start')
    const timer = setInterval(() => response.write('progress'), 5)
    response.once('close', () => clearInterval(timer))
  })
  const response = await host.dispatch({ operation: 'network.request', url, timeoutMs: 1000, totalTimeoutMs: 100 })
  await assert.rejects(readAll(host, response.handle), /deadline timed out/)
})


test('HTTP decompression removes compressed length and encoding headers', async t => {
  const host = createHostServices()
  t.after(() => host.close())
  const content = Buffer.from('streamed metadata'.repeat(100))
  const compressed = gzipSync(content)
  const url = await serve(t, (request, response) => {
    response.writeHead(200, { 'content-encoding': 'gzip', 'content-length': compressed.length })
    response.end(compressed)
  })
  const response = await host.dispatch({ operation: 'network.request', url })
  assert.ok(!response.headers.some(([name]) => ['content-length', 'content-encoding'].includes(name)))
  assert.deepEqual(await readAll(host, response.handle), content)
})

test('process file redirection resolves guest descriptors without closing the caller file', async t => {
  const directory = mkdtempSync(join(tmpdir(), 'pnpm-wasm-process-'))
  const path = join(directory, 'stdout')
  const descriptor = openSync(path, 'w')
  const host = createHostServices({ acquireDescriptor: fd => {
    if (fd !== 42) throw new Error(`Invalid WASI descriptor: ${fd}`)
    return { fd: descriptor, release () {} }
  } })
  t.after(async () => {
    await host.close()
    closeSync(descriptor)
    rmSync(directory, { recursive: true, force: true })
  })
  const child = await host.dispatch({
    operation: 'process.spawn', program: process.execPath, args: ['-e', "process.stdout.write('child')"],
    stdout: { fd: 42 },
  })
  assert.ok(Number.isInteger(child.pid))
  assert.equal(child.stdout, null)
  await host.dispatch({ operation: 'process.wait', handle: child.handle })
  assert.deepEqual(await host.dispatch({ operation: 'process.tryWait', handle: child.handle }), { code: 0, signal: null, signalNumber: null })
  await host.dispatch({ operation: 'resource.close', handle: child.handle })
  writeSync(descriptor, '-parent')
  assert.equal(readFileSync(path, 'utf8'), 'child-parent')
  await assert.rejects(host.dispatch({ operation: 'process.spawn', program: process.execPath, stdout: { fd: 43 } }), /Invalid WASI descriptor/)
})

test('HTTP uploads stream binary bodies larger than the guest transfer limit', async t => {
  const host = createHostServices()
  t.after(() => host.close())
  const expected = Buffer.alloc(2 * 1024 * 1024 + 17, 255)
  const url = await serve(t, async (request, response) => {
    const chunks = []
    for await (const chunk of request) chunks.push(chunk)
    assert.deepEqual(Buffer.concat(chunks), expected)
    response.end(String(expected.length))
  })
  const upload = await host.dispatch({ operation: 'upload.create' })
  const response = host.dispatch({ operation: 'network.request', url, method: 'POST', bodyHandle: upload.handle })
  for (let offset = 0; offset < expected.length; offset += 65536) {
    // eslint-disable-next-line no-await-in-loop -- Each upload write waits for transport backpressure before the next chunk.
    await host.dispatch({ operation: 'upload.write', handle: upload.handle, bytes: expected.subarray(offset, offset + 65536) })
  }
  await host.dispatch({ operation: 'upload.end', handle: upload.handle })
  const result = await response
  assert.equal((await readAll(host, result.handle)).toString(), String(expected.length))
  await host.dispatch({ operation: 'resource.close', handle: upload.handle })
})

test('process wait reports parent exit while a descendant still holds its output pipe', { timeout: 2000 }, async t => {
  const host = createHostServices()
  t.after(() => host.close())
  const child = await host.dispatch({
    operation: 'process.spawn', program: process.execPath, stdout: 'pipe',
    args: ['-e', `
const child = require('node:child_process').spawn(process.execPath, ['-e', 'setTimeout(() => {}, 10000)'], {stdio:'inherit'});
console.log(child.pid);
child.unref();
`],
  })
  const output = await host.dispatch({ operation: 'stream.read', handle: child.stdout })
  const descendant = Number(Buffer.from(output.bytes).toString().trim())
  t.after(() => {
    try { process.kill(descendant, 'SIGKILL') } catch (error) {
      if (error.code !== 'ESRCH') throw error
    }
  })
  const status = await host.dispatch({ operation: 'process.wait', handle: child.handle })
  assert.equal(status.code, 0)
  process.kill(descendant, 0)
})

test('host shutdown rejects an upload writer blocked by unread backpressure', { timeout: 2000 }, async () => {
  const host = createHostServices()
  const upload = await host.dispatch({ operation: 'upload.create' })
  await host.dispatch({ operation: 'upload.write', handle: upload.handle, bytes: Uint8Array.of(1) })
  const pending = host.dispatch({ operation: 'upload.write', handle: upload.handle, bytes: Uint8Array.of(2) })
  const rejected = assert.rejects(pending)
  await host.close()
  await rejected
})


test('process cancellation does not execute a ps binary from project PATH', async t => {
  const directory = mkdtempSync(join(tmpdir(), 'pnpm-wasm-untrusted-path-'))
  const marker = join(directory, 'executed')
  writeFileSync(join(directory, 'ps'), `#!/bin/sh\nprintf compromised > '${marker}'\n`, { mode: 0o755 })
  const originalPath = process.env.PATH
  process.env.PATH = directory
  const host = createHostServices()
  t.after(async () => {
    process.env.PATH = originalPath
    await host.close()
    rmSync(directory, { recursive: true, force: true })
  })
  const child = await host.dispatch({
    operation: 'process.spawn', program: process.execPath,
    args: ['-e', 'setInterval(() => {}, 1000)'],
  })
  await host.dispatch({ operation: 'process.kill', handle: child.handle, signal: 'SIGKILL' })
  assert.equal((await host.dispatch({ operation: 'process.wait', handle: child.handle })).signal, 'SIGKILL')
  assert.throws(() => readFileSync(marker), { code: 'ENOENT' })
})

test('HTTP URL validation never includes URL passwords in errors', async t => {
  const host = createHostServices()
  t.after(() => host.close())
  for (const url of ['http://user:secret-password@127.0.0.1:1/', 'http://user:secret-password@']) {
    // eslint-disable-next-line no-await-in-loop -- Validate each independent malformed authority without opening a network connection.
    await assert.rejects(host.dispatch({ operation: 'network.request', url }), error => {
      assert.ok(!error.message.includes('secret-password'))
      assert.ok(!String(error).includes('secret-password'))
      return true
    })
  }
})
