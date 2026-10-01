import assert from 'node:assert/strict'
import { PassThrough } from 'node:stream'
import { test } from 'node:test'

import { createResources } from './resources.mjs'
import { openTerminal, writeTerminal } from './terminal-session.mjs'
import { prompt } from './terminal.mjs'

function terminalFixture () {
  const input = new PassThrough()
  const output = new PassThrough()
  input.isTTY = output.isTTY = true
  input.setRawMode = value => { input.isRaw = value }
  output.rows = 25
  output.columns = 80
  let displayed = ''
  output.on('data', chunk => { displayed += chunk })
  return { input, output, displayed: () => displayed }
}

test('terminal session queues keys, renders output and restores raw mode and cursor', async () => {
  const fixture = terminalFixture()
  const resources = createResources()
  const opened = await openTerminal(resources, fixture)
  assert.deepEqual({ rows: opened.rows, columns: opened.columns }, { rows: 25, columns: 80 })
  const session = resources.get(opened.handle, 'terminal')
  fixture.input.write(' \x1b[A\x1b[Bj\r\x03')
  assert.deepEqual(await session.read(), { name: 'other', character: ' ' })
  assert.deepEqual(await session.read(), { name: 'up' })
  assert.deepEqual(await session.read(), { name: 'down' })
  assert.deepEqual(await session.read(), { name: 'other', character: 'j' })
  assert.deepEqual(await session.read(), { name: 'enter' })
  assert.deepEqual(await session.read(), { name: 'ctrl-c' })
  await writeTerminal(session, 'choose')
  await resources.closeAll()
  assert.equal(fixture.input.isRaw, false)
  assert.equal(fixture.input.isPaused(), true)
  assert.equal(fixture.input.listenerCount('data'), 0)
  assert.equal(fixture.displayed(), 'choose\x1b[?25h')
})

test('cancelled key reads preserve subsequent input and session close rejects pending reads', async () => {
  const fixture = terminalFixture()
  const resources = createResources()
  const { handle } = await openTerminal(resources, fixture)
  const session = resources.get(handle, 'terminal')
  const controller = new AbortController()
  const cancelled = session.read(controller.signal)
  controller.abort()
  await assert.rejects(cancelled, { name: 'AbortError' })
  fixture.input.write('a')
  assert.deepEqual(await session.read(), { name: 'other', character: 'a' })
  const pending = assert.rejects(session.read(), /Terminal session closed/)
  await resources.closeAll()
  await pending
})

test('line prompts and key sessions share exclusive input ownership', async () => {
  const fixture = terminalFixture()
  const resources = createResources()
  const { handle } = await openTerminal(resources, fixture)
  await assert.rejects(prompt({ message: 'OTP' }, fixture), /already active/)
  await resources.close(handle)
  const pending = prompt({ message: 'Confirm' }, fixture)
  await assert.rejects(openTerminal(resources, fixture), /already active/)
  fixture.input.write('yes\n')
  assert.deepEqual(await pending, { value: 'yes' })
})

test('terminal key queue rejects excess input with bounded storage', async () => {
  const fixture = terminalFixture()
  const resources = createResources()
  const { handle } = await openTerminal(resources, fixture)
  fixture.input.write('a'.repeat(129))
  await assert.rejects(resources.get(handle, 'terminal').read(), /exceeded 128 keys/)
  await resources.closeAll()
})
