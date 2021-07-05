import { cpus } from 'os'
import { getWorkspaceConcurrency } from '../lib/concurrency'

const hostCores = cpus().length

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
