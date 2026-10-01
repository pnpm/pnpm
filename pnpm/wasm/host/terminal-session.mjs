import { emitKeypressEvents } from 'node:readline'
import { PassThrough } from 'node:stream'

import { claimTerminalInput } from './terminal-input.mjs'

export async function openTerminal (resources, { input = process.stdin, output = selectOutput(), maxQueuedKeys = 128 } = {}) {
  if (!input.isTTY || !output?.isTTY || typeof input.setRawMode !== 'function') {
    throw Object.assign(new Error('Interactive terminal input is unavailable'), { code: 'ENOTTY' })
  }
  const release = claimTerminalInput(input)
  let session
  try {
    session = createSession(input, output, { release, maxQueuedKeys })
    return { handle: resources.add(session), rows: output.rows ?? null, columns: output.columns ?? null }
  } catch (error) {
    await session?.close()
    release()
    throw error
  }
}

export function writeTerminal (session, text) {
  if (typeof text !== 'string' || text.length > 65536) throw new TypeError('Invalid terminal output')
  return new Promise((resolve, reject) => {
    session.output.write(text, error => error ? reject(error) : resolve(null))
  })
}

function createSession (input, output, { release, maxQueuedKeys }) {
  const keys = new PassThrough()
  const state = { queue: [], pending: null, error: null, closed: false, maxQueuedKeys }
  const wasRaw = Boolean(input.isRaw)
  const wasFlowing = input.readableFlowing === true
  const onData = chunk => keys.write(chunk)
  const onError = error => fail(state, error)
  const onEnd = () => fail(state, Object.assign(new Error('Terminal input closed'), { code: 'EINTR' }))
  emitKeypressEvents(keys)
  keys.on('keypress', (character, key) => receiveKey(state, decodeKey(character, key)))
  input.setRawMode(true)
  input.on('data', onData).on('error', onError).on('end', onEnd)
  output.on('error', onError)
  input.resume()
  return {
    kind: 'terminal', output,
    read: signal => readKey(state, signal),
    async close () {
      if (state.closed) return
      state.closed = true
      fail(state, new Error('Terminal session closed'))
      input.off('data', onData).off('error', onError).off('end', onEnd)
      keys.destroy()
      try {
        try {
          input.setRawMode(wasRaw)
        } finally {
          if (!wasFlowing) input.pause()
          release()
        }
        await writeTerminal({ output }, '\x1b[?25h')
      } finally {
        output.off('error', onError)
      }
    },
  }
}

function readKey (state, signal) {
  signal?.throwIfAborted()
  if (state.error) return Promise.reject(state.error)
  if (state.pending) throw new Error('A terminal key read is already pending')
  if (state.queue.length) return Promise.resolve(state.queue.shift())
  return new Promise((resolve, reject) => {
    const abort = () => {
      state.pending = null
      reject(signal.reason)
    }
    state.pending = {
      resolve: key => { signal?.removeEventListener('abort', abort); resolve(key) },
      reject: error => { signal?.removeEventListener('abort', abort); reject(error) },
    }
    signal?.addEventListener('abort', abort, { once: true })
  })
}

function receiveKey (state, key) {
  if (state.closed || state.error) return
  if (state.pending) {
    const pending = state.pending
    state.pending = null
    pending.resolve(key)
  } else if (state.queue.length < state.maxQueuedKeys) state.queue.push(key)
  else fail(state, new Error(`Terminal input queue exceeded ${state.maxQueuedKeys} keys`))
}

function fail (state, error) {
  state.error ??= error
  state.queue.length = 0
  state.pending?.reject(state.error)
  state.pending = null
}

function decodeKey (character, key) {
  if (key.ctrl && key.name === 'c') return { name: 'ctrl-c' }
  if (key.ctrl && key.name === 'd') return { name: 'eof' }
  if (['escape', 'backspace', 'delete', 'left', 'right', 'home', 'end'].includes(key.name)) return { name: key.name }
  if (key.name === 'tab') return { name: key.shift ? 'backtab' : 'tab' }
  if (['return', 'enter'].includes(key.name)) return { name: 'enter' }
  if (['up', 'down'].includes(key.name)) return { name: key.name }
  return { name: 'other', ...(!key.ctrl && !key.meta && character ? { character } : {}) }
}

function selectOutput () {
  return process.stdout.isTTY ? process.stdout : process.stderr
}
