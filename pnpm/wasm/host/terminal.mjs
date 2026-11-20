import { createInterface } from 'node:readline'

import { claimTerminalInput } from './terminal-input.mjs'

export async function prompt (options, { signal, input = process.stdin, output = process.stderr } = {}) {
  if (typeof options.message !== 'string') throw new TypeError('Terminal prompt message must be a string')
  signal?.throwIfAborted()
  const release = claimTerminalInput(input)
  let terminal
  let abort
  let keypress
  try {
    terminal = createInterface({ input, output, terminal: Boolean(input.isTTY && output.isTTY) })
    return await new Promise((resolve, reject) => {
      abort = () => reject(signal.reason)
      signal?.addEventListener('abort', abort, { once: true })
      terminal.once('SIGINT', () => resolve({ cancelled: true }))
      keypress = (_text, key) => {
        if (options.cancelOnEscape && key?.name === 'escape') resolve({ cancelled: true })
      }
      input.on('keypress', keypress)
      terminal.once('close', () => resolve({ value: null }))
      terminal.once('error', reject)
      terminal.question(`${options.message}: `, value => resolve({ value }))
    })
  } finally {
    signal?.removeEventListener('abort', abort)
    if (keypress) input.off('keypress', keypress)
    terminal?.close()
    release()
  }
}

export async function confirm (options, context = {}) {
  const input = context.input ?? process.stdin
  const output = context.output ?? process.stderr
  if (!input.isTTY || !output.isTTY) throw Object.assign(new Error('Confirmation requires an interactive terminal'), { code: 'ENOTTY' })
  if (options.default != null && typeof options.default !== 'boolean') throw new TypeError('Confirmation default must be a boolean')
  const suffix = options.default == null ? '(y/n)' : options.default ? '(Y/n)' : '(y/N)'
  while (true) {
    // eslint-disable-next-line no-await-in-loop -- each answer determines whether another prompt is needed
    const answer = await prompt({ message: `${options.message} ${suffix}`, cancelOnEscape: true }, { ...context, input, output })
    if (answer.cancelled || answer.value == null) throw Object.assign(new Error('Confirmation cancelled'), { code: 'EINTR' })
    const value = answer.value.trim().toLowerCase()
    if (value === 'y' || value === 'yes') return true
    if (value === 'n' || value === 'no') return false
    if (value === '' && options.default != null) return options.default
  }
}

export async function input (options, context = {}) {
  if (!(context.input ?? process.stdin).isTTY || !(context.output ?? process.stderr).isTTY) {
    throw Object.assign(new Error('Input requires an interactive terminal'), { code: 'ENOTTY' })
  }
  while (true) {
    // eslint-disable-next-line no-await-in-loop -- empty input must be retried when the caller requires a value
    const answer = await prompt({ message: options.message, cancelOnEscape: true }, context)
    if (answer.cancelled || answer.value == null) throw Object.assign(new Error('Input cancelled'), { code: 'EINTR' })
    if (options.allowEmpty || answer.value !== '') return answer.value
  }
}
