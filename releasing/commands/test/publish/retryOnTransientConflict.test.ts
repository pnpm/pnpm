import { describe, expect, jest, test } from '@jest/globals'

import {
  retryOnTransientConflict,
  type TransientConflictRetryContext,
} from '../../src/publish/retryOnTransientConflict.js'

function createContext (overrides?: Partial<TransientConflictRetryContext>): TransientConflictRetryContext {
  return {
    setTimeout: (cb: () => void) => cb(),
    globalInfo: () => {},
    ...overrides,
  }
}

describe('retryOnTransientConflict', () => {
  test('returns the result when the operation succeeds on the first attempt', async () => {
    const operation = jest.fn(async () => 'ok')
    const result = await retryOnTransientConflict({
      context: createContext(),
      operation,
    })
    expect(result).toBe('ok')
    expect(operation).toHaveBeenCalledTimes(1)
  })

  test('retries the operation when a 409 statusCode error is thrown', async () => {
    let callCount = 0
    const operation = jest.fn(async () => {
      callCount++
      if (callCount < 3) {
        throw Object.assign(new Error('Conflict'), { statusCode: 409 })
      }
      return 'success'
    })
    const result = await retryOnTransientConflict({
      context: createContext(),
      config: { retries: 5 },
      operation,
    })
    expect(result).toBe('success')
    expect(operation).toHaveBeenCalledTimes(3)
  })

  test('retries the operation when an E409 code error is thrown', async () => {
    let callCount = 0
    const operation = jest.fn(async () => {
      callCount++
      if (callCount === 1) {
        throw Object.assign(new Error('Conflict'), { code: 'E409' })
      }
      return 'success'
    })
    const result = await retryOnTransientConflict({
      context: createContext(),
      operation,
    })
    expect(result).toBe('success')
    expect(operation).toHaveBeenCalledTimes(2)
  })

  test('rethrows the 409 error after exhausting retries', async () => {
    const error = Object.assign(new Error('Conflict'), { statusCode: 409 })
    const operation = jest.fn(async () => {
      throw error
    })
    await expect(retryOnTransientConflict({
      context: createContext(),
      config: { retries: 2 },
      operation,
    })).rejects.toBe(error)
    expect(operation).toHaveBeenCalledTimes(3)
  })

  test('rethrows non-conflict errors immediately without retrying', async () => {
    const error = Object.assign(new Error('Server error'), { statusCode: 500 })
    const operation = jest.fn(async () => {
      throw error
    })
    await expect(retryOnTransientConflict({
      context: createContext(),
      operation,
    })).rejects.toBe(error)
    expect(operation).toHaveBeenCalledTimes(1)
  })

  test('rethrows plain errors without statusCode immediately', async () => {
    const error = new Error('Network failure')
    const operation = jest.fn(async () => {
      throw error
    })
    await expect(retryOnTransientConflict({
      context: createContext(),
      operation,
    })).rejects.toBe(error)
    expect(operation).toHaveBeenCalledTimes(1)
  })

  test('uses exponential backoff between retries', async () => {
    const delays: number[] = []
    let callCount = 0
    const operation = jest.fn(async () => {
      callCount++
      if (callCount < 4) {
        throw Object.assign(new Error('Conflict'), { statusCode: 409 })
      }
      return 'ok'
    })
    await retryOnTransientConflict({
      context: createContext({
        setTimeout: (cb, ms) => {
          delays.push(ms)
          cb()
        },
      }),
      config: { retries: 5, factor: 2, minTimeout: 1000, maxTimeout: 10_000 },
      operation,
    })
    expect(delays).toStrictEqual([1000, 2000, 4000])
  })

  test('clamps backoff to maxTimeout', async () => {
    const delays: number[] = []
    let callCount = 0
    const operation = jest.fn(async () => {
      callCount++
      if (callCount < 4) {
        throw Object.assign(new Error('Conflict'), { statusCode: 409 })
      }
      return 'ok'
    })
    await retryOnTransientConflict({
      context: createContext({
        setTimeout: (cb, ms) => {
          delays.push(ms)
          cb()
        },
      }),
      config: { retries: 5, factor: 10, minTimeout: 5000, maxTimeout: 8000 },
      operation,
    })
    expect(delays).toStrictEqual([5000, 8000, 8000])
  })

  test('logs informational messages when retrying', async () => {
    const globalInfo = jest.fn<TransientConflictRetryContext['globalInfo']>()
    let callCount = 0
    const operation = async () => {
      callCount++
      if (callCount < 2) {
        throw Object.assign(new Error('Conflict'), { statusCode: 409 })
      }
      return 'ok'
    }
    await retryOnTransientConflict({
      context: createContext({ globalInfo }),
      config: { retries: 3, minTimeout: 1000, factor: 2 },
      operation,
    })
    expect(globalInfo).toHaveBeenCalledTimes(1)
    expect(globalInfo).toHaveBeenCalledWith(expect.stringContaining('409 Conflict'))
  })

  test('does not retry when retries is 0', async () => {
    const error = Object.assign(new Error('Conflict'), { statusCode: 409 })
    const operation = jest.fn(async () => {
      throw error
    })
    await expect(retryOnTransientConflict({
      context: createContext(),
      config: { retries: 0 },
      operation,
    })).rejects.toBe(error)
    expect(operation).toHaveBeenCalledTimes(1)
  })
})
