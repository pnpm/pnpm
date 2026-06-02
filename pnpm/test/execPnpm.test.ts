import type { ChildProcess } from 'node:child_process'

import { afterEach, expect, jest, test } from '@jest/globals'

import { registerProcessTimeout } from './utils/execPnpm.js'

afterEach(() => {
  jest.useRealTimers()
})

test('kills a process that does not exit gracefully after timing out', () => {
  jest.useFakeTimers()
  const proc = {
    exitCode: null as number | null,
    signalCode: null as NodeJS.Signals | null,
    kill: jest.fn((_signal?: NodeJS.Signals | number) => true),
  }
  const onTimeout = jest.fn()

  registerProcessTimeout(proc as unknown as ChildProcess, 100, onTimeout)

  jest.advanceTimersByTime(100)
  expect(onTimeout).toHaveBeenCalledWith(new Error('Command timed out after 100ms'))
  expect(proc.kill).toHaveBeenCalledWith('SIGINT')

  jest.advanceTimersByTime(10_000)
  expect(proc.kill).toHaveBeenCalledTimes(2)
  expect(proc.kill).toHaveBeenLastCalledWith()
})

test('does not kill a process again if it exits gracefully after timing out', () => {
  jest.useFakeTimers()
  const proc = {
    exitCode: null as number | null,
    signalCode: null as NodeJS.Signals | null,
    kill: jest.fn((_signal?: NodeJS.Signals | number) => true),
  }

  registerProcessTimeout(proc as unknown as ChildProcess, 100, jest.fn())

  jest.advanceTimersByTime(100)
  proc.exitCode = 0
  jest.advanceTimersByTime(10_000)
  expect(proc.kill).toHaveBeenCalledTimes(1)
})

test('does not kill a process again if it receives a signal after timing out', () => {
  jest.useFakeTimers()
  const proc = {
    exitCode: null as number | null,
    kill: jest.fn((_signal?: NodeJS.Signals | number) => true),
    signalCode: null as NodeJS.Signals | null,
  }
  const onTimeout = jest.fn()

  registerProcessTimeout(proc as unknown as ChildProcess, 100, onTimeout)

  jest.advanceTimersByTime(100)
  expect(onTimeout).toHaveBeenCalledWith(new Error('Command timed out after 100ms'))
  expect(proc.kill).toHaveBeenCalledWith('SIGINT')

  proc.signalCode = 'SIGINT'
  jest.advanceTimersByTime(10_000)
  expect(proc.kill).toHaveBeenCalledTimes(1)
})
