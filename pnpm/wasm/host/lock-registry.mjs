import { randomUUID } from 'node:crypto'
import fs from 'node:fs'
import { createConnection, createServer } from 'node:net'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'

const mutexPort = 29753
const tokens = new Set()
let listener
let acquiring = 0

// The rendezvous port protects only registry updates. Held file locks use
// ephemeral ports, so unrelated nested locks cannot collide or reverse order.
export async function acquireFileLease (identity, exclusive, previous) {
  acquiring++
  try {
    return await acquireRegisteredLease(identity, exclusive, previous)
  } finally {
    acquiring--
    closeIdleListener()
  }
}

async function acquireRegisteredLease (identity, exclusive, previous) {
  const mutex = await listen(createServer(socket => socket.destroy()), mutexPort)
  if (!mutex) return undefined
  let token
  try {
    const file = registryPath(identity)
    const owners = await liveOwners(file)
    if (owners.some(owner => owner.token !== previous?.token && (exclusive || owner.exclusive))) return undefined
    const server = await leaseListener()
    token = previous?.token ?? randomUUID()
    tokens.add(token)
    const owner = { port: server.address().port, token, exclusive, pid: process.pid }
    writeOwners(file, [...owners.filter(entry => entry.token !== token), owner])
    return { token, exclusive, release () { tokens.delete(token); closeIdleListener() } }
  } catch (error) {
    if (token && token !== previous?.token) tokens.delete(token)
    throw error
  } finally {
    await new Promise(resolve => mutex.close(resolve))
  }
}

function registryPath (identity) {
  const directory = path.join(os.tmpdir(), `pnpm-wasm-file-locks-${process.getuid()}`)
  fs.mkdirSync(directory, { recursive: true, mode: 0o700 })
  const descriptor = fs.openSync(directory, fs.constants.O_RDONLY | fs.constants.O_DIRECTORY | fs.constants.O_NOFOLLOW)
  try {
    if (fs.fstatSync(descriptor).uid !== process.getuid()) throw new Error('WASM lock registry belongs to another user')
    fs.fchmodSync(descriptor, 0o700)
  } finally {
    fs.closeSync(descriptor)
  }
  return path.join(directory, identity.replace(':', '-'))
}

async function liveOwners (file) {
  let descriptor
  try {
    descriptor = fs.openSync(file, fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW | fs.constants.O_NONBLOCK)
  } catch (error) {
    if (error.code === 'ENOENT') return []
    throw error
  }
  let owners
  try {
    const metadata = fs.fstatSync(descriptor)
    if (!metadata.isFile() || metadata.size > 1024 * 1024) throw new Error('Invalid WASM lock ownership record')
    owners = JSON.parse(fs.readFileSync(descriptor, 'utf8'))
  } finally {
    fs.closeSync(descriptor)
  }
  if (!Array.isArray(owners) || owners.length > 4096 || !owners.every(validOwner)) throw new Error('Invalid WASM lock ownership record')
  const live = await Promise.all(owners.map(async owner => await isAlive(owner) ? owner : undefined))
  return live.filter(Boolean)
}

function validOwner (owner) {
  return Number.isInteger(owner?.port) && owner.port > 0 && owner.port < 65536 &&
    typeof owner.token === 'string' && /^[\da-f-]{36}$/.test(owner.token) && typeof owner.exclusive === 'boolean' &&
    (owner.pid == null || (Number.isSafeInteger(owner.pid) && owner.pid > 0))
}

function writeOwners (file, owners) {
  const temporary = `${file}.${randomUUID()}`
  try {
    fs.writeFileSync(temporary, JSON.stringify(owners), { flag: 'wx', mode: 0o600 })
    fs.renameSync(temporary, file)
  } catch (error) {
    try { fs.unlinkSync(temporary) } catch (cleanup) { if (cleanup.code !== 'ENOENT') throw cleanup }
    throw error
  }
}

async function leaseListener () {
  listener ??= listen(createServer(socket => {
    let request = ''
    socket.setTimeout(1000, () => socket.destroy())
    socket.on('error', () => { /* A probing process may exit before receiving the response. */ })
    socket.on('data', chunk => {
      request += chunk.toString()
      if (request.length > 37) return socket.destroy()
      if (request.endsWith('\n')) socket.end(tokens.has(request.trim()) ? 'alive\n' : 'released\n')
    })
  }), 0)
  return listener
}

function listen (server, port) {
  return new Promise((resolve, reject) => {
    server.once('error', error => {
      if (error.code === 'EADDRINUSE') resolve(undefined)
      else reject(error)
    })
    server.listen({ host: '127.0.0.1', port, exclusive: true }, () => {
      server.unref()
      resolve(server)
    })
  })
}

function isAlive (owner) {
  if (owner.pid != null && processIsAbsent(owner.pid)) return Promise.resolve(false)
  return new Promise((resolve, reject) => {
    const socket = createConnection({ host: '127.0.0.1', port: owner.port })
    let response = ''
    function finish (value) {
      socket.destroy()
      resolve(value)
    }
    // A slow live owner must never be reaped. Only refusal or an explicit
    // different token proves that this ownership record is abandoned.
    socket.setTimeout(1000, () => finish(true))
    socket.on('connect', () => socket.write(`${owner.token}\n`))
    socket.on('data', chunk => {
      response += chunk.toString()
      if (response.endsWith('\n') || response.length > 32) finish(response !== 'released\n')
    })
    socket.on('end', () => finish(response !== 'released\n'))
    socket.on('error', error => {
      socket.destroy()
      if (error.code === 'ECONNREFUSED') resolve(false)
      else if (error.code === 'ECONNRESET') resolve(true)
      else reject(error)
    })
  })
}

function processIsAbsent (pid) {
  try {
    process.kill(pid, 0)
  } catch (error) {
    if (error.code === 'ESRCH') return true
    if (!['EPERM', 'EINVAL', 'ENOSYS'].includes(error.code)) throw error
  }
  return false
}

function closeIdleListener () {
  if (tokens.size || acquiring || !listener) return
  const closing = listener
  listener = undefined
  void closing.then(server => server.close(), () => { /* The acquisition already reports a failed listener. */ })
}
