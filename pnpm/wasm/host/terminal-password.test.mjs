import assert from 'node:assert/strict'
import { PassThrough } from 'node:stream'
import { test } from 'node:test'

import { password } from './terminal-password.mjs'
import { input } from './terminal.mjs'

function fixture () {
  const source = Object.assign(new PassThrough(), { isTTY: true })
  const output = Object.assign(new PassThrough(), { isTTY: true })
  source.setRawMode = value => { source.isRaw = value }
  let displayed = ''
  output.on('data', chunk => { displayed += chunk })
  return { input: source, output, displayed: () => displayed }
}

test('password input never echoes secrets and restores raw mode after editing', async () => {
  const terminal = fixture()
  const answer = password({ message: 'Password', allowEmpty: false }, terminal)
  terminal.input.write('very-secretx\x7f\r')
  assert.equal(await answer, 'very-secret')
  assert.equal(terminal.displayed(), 'Password: \n\x1b[?25h')
  assert.equal(terminal.input.isRaw, false)
  assert.equal(terminal.input.listenerCount('data'), 0)
})

test('password accepts empty values only when allowed and handles long pasted credentials', async () => {
  const terminal = fixture()
  const empty = password({ message: 'Password', allowEmpty: true }, terminal)
  terminal.input.write('\r')
  assert.equal(await empty, '')
  const required = password({ message: 'Password', allowEmpty: false }, terminal)
  const secret = 'a'.repeat(300)
  terminal.input.write(`\r${secret}\r`)
  assert.equal(await required, secret)
  assert.equal(terminal.displayed().includes(secret), false)
})

test('password abort, EOF, Escape and Ctrl-C release terminal ownership without leaking input', async () => {
  await Promise.all([null, '\x03', '\x1b', 'abort'].map(async cancellation => {
    const terminal = fixture()
    const controller = new AbortController()
    const answer = password({ message: 'Password' }, { ...terminal, signal: controller.signal })
    terminal.input.write('hidden')
    if (cancellation === 'abort') controller.abort()
    else if (cancellation) terminal.input.write(cancellation)
    else terminal.input.end()
    await assert.rejects(answer)
    assert.equal(terminal.input.isRaw, false)
    assert.equal(terminal.displayed().includes('hidden'), false)
  }))
})

test('visible input preserves text, empty policy and exclusive password ownership', async () => {
  const terminal = fixture()
  const secret = password({ message: 'Password' }, terminal)
  await assert.rejects(input({ message: 'Username', allowEmpty: true }, terminal), /already active/)
  terminal.input.write('secret\r')
  await secret
  const answer = input({ message: 'Username', allowEmpty: false }, terminal)
  terminal.input.write('\r')
  await new Promise(resolve => setImmediate(resolve))
  terminal.input.write('user\r')
  assert.equal(await answer, 'user')
})
