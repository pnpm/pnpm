import assert from 'node:assert/strict'

import { createHostServices } from './runtime/host/operations.mjs'

const host = createHostServices()
const raw = Boolean(process.stdin.isRaw)
try {
  const status = await host.dispatch({ operation: 'terminal.status' })
  assert.equal(status.stdin, true)
  const session = await host.dispatch({ operation: 'terminal.open' })
  process.stdin.emit('data', Buffer.from('\x1b[A \r\x03'))
  const expected = [{ name: 'up' }, { name: 'other', character: ' ' }, { name: 'enter' }, { name: 'ctrl-c' }]
  for (const key of expected) {
    assert.deepEqual(await host.dispatch({ operation: 'terminal.readKey', handle: session.handle }), key)
  }
  await host.dispatch({ operation: 'resource.close', handle: session.handle })
  assert.equal(Boolean(process.stdin.isRaw), raw)
  const password = host.dispatch({ operation: 'terminal.password', message: 'Password', allowEmpty: true })
  process.stdin.emit('data', Buffer.from('hidden-secretx\x7f\r'))
  assert.equal(await password, 'hidden-secret')
  assert.equal(Boolean(process.stdin.isRaw), raw)
  console.log('WebContainer terminal mode, key decoding and password input passed')
} finally {
  await host.close()
}
