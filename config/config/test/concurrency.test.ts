import { jest } from '@jest/globals'
import os, { cpus } from 'os'
import { getDefaultWorkspaceConcurrency, resetAvailableParallelismCache, getWorkspaceConcurrency } from '../lib/concurrency.js'

const hostCores = cpus().length

beforeEach(() => {
  resetAvailableParallelismCache()
})

afterEach(() => {
  resetAvailableParallelismCache()
  jest.restoreAllMocks()
})

function mockAvailableParallelism (value: number) {
  if ('availableParallelism' in os) {
    jest.spyOn(os, 'availableParallelism').mockReturnValue(value)
  }
  jest.spyOn(os, 'cpus').mockReturnValue(Array(value).fill(cpus()[0]))
}

test('getDefaultWorkspaceConcurrency: cpu num < 4', () => {
  mockAvailableParallelism(1)
  expect(getDefaultWorkspaceConcurrency(false)).toBe(1)
})

test('getDefaultWorkspaceConcurrency: cpu num > 4', () => {
  mockAvailableParallelism(5)
  expect(getDefaultWorkspaceConcurrency(false)).toBe(4)
})

test('getDefaultWorkspaceConcurrency: cpu num = 4', () => {
  mockAvailableParallelism(4)
  expect(getDefaultWorkspaceConcurrency(false)).toBe(4)
})

test('getDefaultWorkspaceConcurrency: using cache', () => {
  mockAvailableParallelism(4)
  expect(getDefaultWorkspaceConcurrency()).toBe(4)

  mockAvailableParallelism(5)
  expect(getDefaultWorkspaceConcurrency()).toBe(4)
})

test('default workspace concurrency', () => {
  const n = getWorkspaceConcurrency(undefined)

  expect(n).toBe(4)
})

test('get back positive amount', () => {
  expect(getWorkspaceConcurrency(5)).toBe(5)
})

test('match host cores amount', () => {
  const n = getWorkspaceConcurrency(0)

  expect(n).toBe(hostCores)
})

test('host cores minus X', () => {
  const n1 = getWorkspaceConcurrency(-1)

  expect(n1).toBe(Math.max(1, hostCores - 1))

  const n2 = getWorkspaceConcurrency(-9999)

  expect(n2).toBe(1)
})
