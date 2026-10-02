import { spawn } from 'node:child_process'
import { once } from 'node:events'
import { constants } from 'node:os'

import { killProcessTree } from './process-tree.mjs'
import { createStream } from './resources.mjs'
import { bridgeDescriptors, prepareStdio } from './process-stdio.mjs'

export async function spawnProcess (resources, options, { signal, acquireDescriptor } = {}) {
  const stdio = prepareStdio(options, acquireDescriptor)
  const child = startProcess(options, stdio)
  const processResource = trackProcess(child, bridgeDescriptors(child, stdio.redirects))
  const abort = () => {
    processResource.cancellation = processResource.close().then(() => signal.reason, error => error)
  }
  signal?.addEventListener('abort', abort, { once: true })
  void processResource.completion.then(() => signal?.removeEventListener('abort', abort))
  const handle = resources.add(processResource)
  const stdout = addPipe(resources, stdio.redirects.has(1) ? null : child.stdout)
  const stderr = addPipe(resources, stdio.redirects.has(2) ? null : child.stderr)
  try {
    await once(child, 'spawn')
    signal?.throwIfAborted()
  } catch (error) {
    resources.remove(handle)
    if (stdout !== null) resources.remove(stdout)
    if (stderr !== null) resources.remove(stderr)
    await processResource.close()
    throw error
  }
  return {
    handle,
    pid: child.pid,
    stdout,
    stderr,
  }
}

export async function writeStdin (resource, bytes) {
  if (resource.inputError) throw resource.inputError
  if (!(bytes instanceof Uint8Array)) throw new TypeError('Process input must be a Uint8Array')
  if (!resource.child.stdin) throw new Error('Process stdin is not piped')
  await new Promise((resolve, reject) => {
    resource.child.stdin.write(bytes, error => error ? reject(error) : resolve())
  })
}

export async function endStdin (resource) {
  if (resource.inputError) throw resource.inputError
  if (!resource.child.stdin) throw new Error('Process stdin is not piped')
  await new Promise((resolve, reject) => {
    resource.child.stdin.end(error => error ? reject(error) : resolve())
  })
}

export async function waitProcess (resource) {
  const result = await resource.completion
  const cancellationError = await resource.cancellation
  if (cancellationError) throw cancellationError
  if (result.error) throw result.error
  return result
}

export function tryWaitProcess (resource) {
  if (resource.status?.error) throw resource.status.error
  return resource.status
}

function addPipe (resources, pipe) {
  if (!pipe) return null
  const iterator = pipe[Symbol.asyncIterator]()
  const next = () => iterator.next().then(result => ({ result }), error => ({ error }))
  let pending = next()
  return resources.add(createStream(async () => {
    const settled = await pending
    if (settled.error) throw settled.error
    if (!settled.result.done) pending = next()
    return settled.result
  }, () => pipe.destroy()))
}

function startProcess (options, stdio) {
  try {
    return spawn(options.program, options.args ?? [], {
      cwd: options.cwd, env: options.env, stdio: stdio.modes,
    })
  } catch (error) {
    stdio.release()
    throw error
  }
}

function trackProcess (child, redirected) {
  const completion = new Promise(resolve => {
    child.once('error', error => resolve({ error }))
    child.once('exit', (code, signal) => resolve(exitStatus(code, signal)))
  }).then(async result => {
    const error = result.error ?? await redirected
    return error ? { error } : result
  })
  const resource = {
    kind: 'process',
    child,
    completion,
    kill: signal => killProcessTree(child, signal),
    inputError: null,
    status: null,
    async close () {
      try {
        await killProcessTree(child, 'SIGKILL')
      } finally {
        child.stdin?.destroy()
        child.stdout?.destroy()
        child.stderr?.destroy()
        await completion
      }
    },
  }
  child.stdin?.on('error', error => { resource.inputError = error })
  void completion.then(result => { resource.status = result })
  return resource
}

function exitStatus (code, signal) {
  return { code, signal, signalNumber: signal === null ? null : constants.signals[signal] ?? null }
}
