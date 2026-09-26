import { spawnSync } from 'node:child_process'

import { expect, test } from '@jest/globals'

test('hashes a large worker-cloned buffer with a small heap', () => {
  const script = `
    import assert from 'node:assert/strict'
    import crypto from 'node:crypto'
    import { hashBuffer } from ${JSON.stringify(new URL('../lib/hashBuffer.js', import.meta.url).href)}

    const buffer = structuredClone(crypto.randomBytes(16 * 1024 * 1024))
    const expected = crypto.createHash('sha512').update(buffer).digest('hex')
    assert.equal(hashBuffer('sha512', buffer), expected)
  `
  const result = spawnSync(process.execPath, ['--max-old-space-size=32', '--input-type=module', '--eval', script], {
    encoding: 'utf8',
  })
  expect(result.status).toBe(0)
})
