import assert from 'node:assert/strict'
import process from 'node:process'
import { test } from 'node:test'

import { hasExecutableMode } from './executable-mode.mjs'

test('executable mode rejects a private non-executable interpreter', () => {
  const metadata = { uid: process.getuid(), gid: process.getgid(), mode: 0o600 }
  assert.equal(hasExecutableMode(metadata), false)
  assert.equal(hasExecutableMode({ ...metadata, mode: 0o700 }), true)
})
