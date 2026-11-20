import os from 'node:os'

import { expect, test } from '@jest/globals'

import { getAvailableParallelism, readCgroupCpuQuota } from '../lib/index.js'

test('getAvailableParallelism is at least 1 and never above the affinity count', () => {
  const parallelism = getAvailableParallelism()
  expect(parallelism).toBeGreaterThanOrEqual(1)
  expect(parallelism).toBeLessThanOrEqual(os.availableParallelism())
})

test('getAvailableParallelism applies the cgroup quota only on Linux', () => {
  const expected = process.platform === 'linux'
    ? Math.min(os.availableParallelism(), readCgroupCpuQuota() ?? Infinity)
    : os.availableParallelism()
  expect(getAvailableParallelism()).toBe(expected)
})
