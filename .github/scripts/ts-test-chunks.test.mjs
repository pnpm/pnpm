import assert from 'node:assert/strict'
import { test } from 'node:test'

import { selectChunk, taskWeight } from './ts-test-chunks.mjs'

test('recorded durations spread slow test files that are small on disk', () => {
  const durations = { fallbackSeconds: 1, files: { 'slow-a.ts': 100, 'slow-b.ts': 90 } }
  const tasks = [
    { id: 'big-fast.ts', size: 1000 },
    { id: 'slow-a.ts', size: 300 },
    { id: 'slow-b.ts', size: 200 },
    { id: 'small-fast.ts', size: 100 },
  ].map((task) => ({ ...task, weight: taskWeight(task, durations) }))
  const chunkOf = (id) => [1, 2].find((chunk) => selectChunk(tasks, { chunk, chunks: 2 }).some((task) => task.id === id))

  assert.notEqual(chunkOf('slow-a.ts'), chunkOf('slow-b.ts'))
})
