import { afterEach, beforeEach, expect, jest, test } from '@jest/globals'

const getAvailableParallelism = jest.fn<() => number>()
jest.unstable_mockModule('@pnpm/os.available-parallelism', () => ({ getAvailableParallelism }))

const { getDefaultWorkspaceConcurrency, getWorkspaceConcurrency, resetAvailableParallelismCache } = await import('../lib/concurrency.js')

beforeEach(() => {
  resetAvailableParallelismCache()
  getAvailableParallelism.mockReturnValue(8)
})

afterEach(() => {
  resetAvailableParallelismCache()
  getAvailableParallelism.mockReset()
})

test('getDefaultWorkspaceConcurrency: cpu num < 4', () => {
  getAvailableParallelism.mockReturnValue(1)
  expect(getDefaultWorkspaceConcurrency(false)).toBe(1)
})

test('getDefaultWorkspaceConcurrency: cpu num > 4', () => {
  getAvailableParallelism.mockReturnValue(5)
  expect(getDefaultWorkspaceConcurrency(false)).toBe(4)
})

test('getDefaultWorkspaceConcurrency: cpu num = 4', () => {
  getAvailableParallelism.mockReturnValue(4)
  expect(getDefaultWorkspaceConcurrency(false)).toBe(4)
})

test('getDefaultWorkspaceConcurrency: using cache', () => {
  getAvailableParallelism.mockReturnValue(4)
  expect(getDefaultWorkspaceConcurrency()).toBe(4)

  getAvailableParallelism.mockReturnValue(5)
  expect(getDefaultWorkspaceConcurrency()).toBe(4)
})

test('default workspace concurrency', () => {
  expect(getWorkspaceConcurrency(undefined)).toBe(4)
})

test('get back positive amount', () => {
  expect(getWorkspaceConcurrency(5)).toBe(5)
})

test('zero means the available CPU count', () => {
  expect(getWorkspaceConcurrency(0)).toBe(8)
})

test('available CPUs minus X, at least 1', () => {
  expect(getWorkspaceConcurrency(-1)).toBe(7)
  expect(getWorkspaceConcurrency(-9999)).toBe(1)
})
