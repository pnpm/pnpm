import assert from 'node:assert/strict'
import { PassThrough } from 'node:stream'
import test from 'node:test'

import { confirm, prompt } from './terminal.mjs'

test('terminal prompt returns submitted text and releases stdin', async () => {
  const input = new PassThrough()
  const output = new PassThrough()
  let displayed = ''
  output.on('data', chunk => { displayed += chunk })
  const pending = prompt({ message: 'One-time password' }, { input, output })
  input.write('123456\n')
  assert.deepEqual(await pending, { value: '123456' })
  assert.equal(displayed, 'One-time password: ')
  assert.equal(input.listenerCount('data'), 0)
})

test('cancelling a prompt releases stdin for the next prompt', async () => {
  const input = new PassThrough()
  const output = new PassThrough()
  const controller = new AbortController()
  const pending = prompt({ message: 'One-time password' }, { input, output, signal: controller.signal })
  controller.abort()
  await assert.rejects(pending, { name: 'AbortError' })
  assert.equal(input.listenerCount('data'), 0)
  const next = prompt({ message: 'Retry' }, { input, output })
  input.write('654321\n')
  assert.deepEqual(await next, { value: '654321' })
})

test('closing stdin resolves a prompt with no value', async () => {
  const input = new PassThrough()
  const pending = prompt({ message: 'One-time password' }, { input, output: new PassThrough() })
  input.end()
  assert.deepEqual(await pending, { value: null })
})

test('concurrent prompts cannot consume each others input', async () => {
  const input = new PassThrough()
  const output = new PassThrough()
  const pending = prompt({ message: 'First' }, { input, output })
  await assert.rejects(prompt({ message: 'Second' }, { input, output }), /already active/)
  input.write('first\n')
  assert.deepEqual(await pending, { value: 'first' })
})

test('confirmation preserves explicit defaults and requires an interactive terminal', async () => {
  await Promise.all([false, true].map(async defaultValue => {
    const input = Object.assign(new PassThrough(), { isTTY: true })
    const output = Object.assign(new PassThrough(), { isTTY: true })
    const pending = confirm({ message: 'Proceed?', default: defaultValue }, { input, output })
    input.write('\r')
    assert.equal(await pending, defaultValue)
  }))
  await assert.rejects(confirm({ message: 'Proceed?', default: true }, {
    input: new PassThrough(), output: new PassThrough(),
  }), { code: 'ENOTTY' })
})

test('confirmation retries invalid input and never approves cancellation or EOF', async () => {
  const input = Object.assign(new PassThrough(), { isTTY: true })
  const output = Object.assign(new PassThrough(), { isTTY: true })
  const pending = confirm({ message: 'Proceed?', default: false }, { input, output })
  input.write('unexpected\r')
  await new Promise(resolve => setImmediate(resolve))
  input.write('yes\r')
  assert.equal(await pending, true)
  await Promise.all([null, '\x03', '\x1b'].map(async cancellation => {
    const source = Object.assign(new PassThrough(), { isTTY: true })
    const answer = confirm({ message: 'Proceed?', default: true }, { input: source, output })
    if (cancellation) source.write(cancellation)
    else source.end()
    await assert.rejects(answer, { code: 'EINTR' })
  }))
})
