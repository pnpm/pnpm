import { jest } from '@jest/globals'
import {
  type EnterKeyListener,
  pollWithBrowserOpen,
  type PollWithBrowserOpenContext,
} from '@pnpm/network.web-auth'

function createDeferred<T> (): {
  promise: Promise<T>
  resolve: (value: T) => void
  reject: (reason?: unknown) => void
} {
  let resolve!: (value: T) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((res, rej) => {
    resolve = res
    reject = rej
  })
  return { promise, resolve, reject }
}

function createMockContext (overrides?: Partial<PollWithBrowserOpenContext>): PollWithBrowserOpenContext {
  return {
    globalInfo: () => {},
    globalWarn: () => {},
    ...overrides,
  }
}

function createMockListener (): EnterKeyListener & { simulateEnter: () => void } {
  let resolveEnter!: () => void
  const enterPromise = new Promise<void>(resolve => {
    resolveEnter = resolve
  })
  return {
    enterPromise,
    cleanup: jest.fn<() => void>(),
    simulateEnter: () => resolveEnter(),
  }
}

describe('pollWithBrowserOpen', () => {
  it('returns the poll result when poll completes before Enter', async () => {
    const listener = createMockListener()
    const context = createMockContext({
      listenForEnter: () => listener,
      openBrowser: jest.fn<(url: string) => Promise<void>>().mockResolvedValue(undefined),
    })

    const token = await pollWithBrowserOpen({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: Promise.resolve('my-token'),
    })

    expect(token).toBe('my-token')
    expect(listener.cleanup).toHaveBeenCalled()
    expect(context.openBrowser).not.toHaveBeenCalled()
  })

  it('opens browser when Enter is pressed before poll completes', async () => {
    const listener = createMockListener()
    const pollDeferred = createDeferred<string>()
    const openBrowser = jest.fn<(url: string) => Promise<void>>().mockResolvedValue(undefined)

    const context = createMockContext({
      listenForEnter: () => listener,
      openBrowser,
    })

    const resultPromise = pollWithBrowserOpen({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: pollDeferred.promise,
    })

    // Simulate Enter press
    listener.simulateEnter()

    // Give the microtask queue time to process the .then()
    await new Promise<void>(resolve => queueMicrotask(resolve))

    expect(openBrowser).toHaveBeenCalledWith('https://example.com/auth')

    // Now resolve the poll
    pollDeferred.resolve('token-after-enter')
    const token = await resultPromise

    expect(token).toBe('token-after-enter')
    expect(listener.cleanup).toHaveBeenCalled()
  })

  it('warns and continues polling when openBrowser fails', async () => {
    const listener = createMockListener()
    const pollDeferred = createDeferred<string>()
    const globalWarn = jest.fn<(msg: string) => void>()
    const globalInfo = jest.fn<(msg: string) => void>()

    const context = createMockContext({
      listenForEnter: () => listener,
      openBrowser: jest.fn<(url: string) => Promise<void>>().mockRejectedValue(new Error('xdg-open not found')),
      globalWarn,
      globalInfo,
    })

    const resultPromise = pollWithBrowserOpen({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: pollDeferred.promise,
    })

    listener.simulateEnter()

    // Let the .then() and .catch() propagate
    await new Promise<void>(resolve => queueMicrotask(resolve))
    await new Promise<void>(resolve => queueMicrotask(resolve))

    expect(globalWarn).toHaveBeenCalledWith(expect.stringContaining('xdg-open not found'))
    expect(globalInfo).toHaveBeenCalledWith('Please open the URL shown above manually.')

    pollDeferred.resolve('tok')
    expect(await resultPromise).toBe('tok')
  })

  it('warns and falls back to plain poll when listenForEnter throws', async () => {
    const globalWarn = jest.fn<(msg: string) => void>()
    const context = createMockContext({
      listenForEnter: () => {
        throw new Error('setRawMode not supported')
      },
      openBrowser: jest.fn<(url: string) => Promise<void>>(),
      globalWarn,
    })

    const token = await pollWithBrowserOpen({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: Promise.resolve('fallback-token'),
    })

    expect(token).toBe('fallback-token')
    expect(globalWarn).toHaveBeenCalledWith(expect.stringContaining('setRawMode not supported'))
    expect(context.openBrowser).not.toHaveBeenCalled()
  })

  it('falls back to plain poll when listenForEnter is not provided', async () => {
    const context = createMockContext({
      openBrowser: jest.fn<(url: string) => Promise<void>>(),
    })

    const token = await pollWithBrowserOpen({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: Promise.resolve('plain-token'),
    })

    expect(token).toBe('plain-token')
    expect(context.openBrowser).not.toHaveBeenCalled()
  })

  it('falls back to plain poll when openBrowser is not provided', async () => {
    const listener = createMockListener()
    const context = createMockContext({
      listenForEnter: () => listener,
    })

    const token = await pollWithBrowserOpen({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: Promise.resolve('plain-token'),
    })

    expect(token).toBe('plain-token')
    expect(listener.cleanup).not.toHaveBeenCalled()
  })

  it('shows the press-Enter message', async () => {
    const listener = createMockListener()
    const globalInfo = jest.fn<(msg: string) => void>()
    const context = createMockContext({
      listenForEnter: () => listener,
      openBrowser: jest.fn<(url: string) => Promise<void>>().mockResolvedValue(undefined),
      globalInfo,
    })

    await pollWithBrowserOpen({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: Promise.resolve('tok'),
    })

    expect(globalInfo).toHaveBeenCalledWith('Press ENTER to open in browser...')
  })

  it('cleans up when poll rejects', async () => {
    const listener = createMockListener()
    const context = createMockContext({
      listenForEnter: () => listener,
      openBrowser: jest.fn<(url: string) => Promise<void>>().mockResolvedValue(undefined),
    })

    await expect(pollWithBrowserOpen({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: Promise.reject(new Error('timeout')),
    })).rejects.toThrow('timeout')

    expect(listener.cleanup).toHaveBeenCalled()
  })
})
