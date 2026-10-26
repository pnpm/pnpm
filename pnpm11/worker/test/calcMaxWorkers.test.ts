import { afterEach, expect, jest, test } from '@jest/globals'

const { calcMaxWorkers } = await import('../lib/index.js')

const originalMaxWorkers = process.env.PNPM_MAX_WORKERS
const originalWorkers = process.env.PNPM_WORKERS

afterEach(() => {
  if (originalMaxWorkers === undefined) {
    delete process.env.PNPM_MAX_WORKERS
  } else {
    process.env.PNPM_MAX_WORKERS = originalMaxWorkers
  }
  if (originalWorkers === undefined) {
    delete process.env.PNPM_WORKERS
  } else {
    process.env.PNPM_WORKERS = originalWorkers
  }
  jest.restoreAllMocks()
})

test('sizes the worker pool from the available CPU count', () => {
  process.env.PNPM_MAX_WORKERS = ''
  delete process.env.PNPM_WORKERS

  expect(calcMaxWorkers(4)).toBe(3)
})

test('bounds the default worker pool when the CPU quota is unavailable', () => {
  process.env.PNPM_MAX_WORKERS = ''
  delete process.env.PNPM_WORKERS

  expect(calcMaxWorkers(233)).toBe(16)
})

test('applies PNPM_WORKERS to the cgroup CPU quota', () => {
  delete process.env.PNPM_MAX_WORKERS
  process.env.PNPM_WORKERS = '2'
  expect(calcMaxWorkers(4)).toBe(1)
})

test('keeps PNPM_MAX_WORKERS as an explicit override', () => {
  process.env.PNPM_MAX_WORKERS = '12'
  delete process.env.PNPM_WORKERS

  expect(calcMaxWorkers(4)).toBe(12)
})
