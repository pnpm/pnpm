import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'

const wrapperDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

test('the placeholder is not a Node.js script', () => {
  assert.doesNotMatch(fs.readFileSync(path.join(wrapperDir, 'pnpm'), 'utf8'), /^#!/)
})
