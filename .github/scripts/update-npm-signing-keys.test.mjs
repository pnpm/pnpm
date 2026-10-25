import assert from 'node:assert/strict'
import fs from 'node:fs'
import { describe, test } from 'node:test'

import { KEYS_FILE, mergeKeys, renderKeys } from './update-npm-signing-keys.mjs'

const key = (keyid, extra = {}) => ({ expires: null, keyid, keytype: 'ecdsa-sha2-nistp256', scheme: 'ecdsa-sha2-nistp256', key: `key-${keyid}`, ...extra })

describe('mergeKeys', () => {
  test('takes the keys npm advertises, sorted by key ID', () => {
    assert.deepEqual(mergeKeys([], [key('SHA256:b'), key('SHA256:a')]), [key('SHA256:a'), key('SHA256:b')])
  })

  test('keeps a pinned key npm no longer lists, marked withdrawn', () => {
    assert.deepEqual(mergeKeys([key('SHA256:old')], [key('SHA256:new')]), [
      key('SHA256:new'),
      key('SHA256:old', { withdrawn: true }),
    ])
  })

  test('drops the withdrawn mark when npm lists the key again', () => {
    assert.deepEqual(mergeKeys([key('SHA256:a', { withdrawn: true })], [key('SHA256:a')]), [key('SHA256:a')])
  })

  test('takes npm\'s copy of a key, with an expiry it added', () => {
    assert.deepEqual(mergeKeys([key('SHA256:a')], [key('SHA256:a', { expires: '2030-01-01T00:00:00.000Z' })]), [
      key('SHA256:a', { expires: '2030-01-01T00:00:00.000Z' }),
    ])
  })

  test('ignores fields npm adds that are not pinned', () => {
    assert.deepEqual(mergeKeys([], [key('SHA256:a', { comment: 'x' })]), [key('SHA256:a')])
  })
})

test('the committed file is in the form the script writes', () => {
  const content = fs.readFileSync(KEYS_FILE, 'utf8')
  assert.equal(renderKeys(JSON.parse(content)), content)
})
