import { beforeEach, describe, expect, it, jest } from '@jest/globals'
import type {
  PromptBrowserOpenContext,
  PromptBrowserOpenReadlineInterface,
} from '@pnpm/network.web-auth'

const mockOpen = jest.fn<(target: string) => Promise<unknown>>()
jest.unstable_mockModule('open', () => ({
  default: mockOpen,
}))

const { promptBrowserOpen } = await import('@pnpm/network.web-auth')

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

interface MockReadlineInterface extends PromptBrowserOpenReadlineInterface {
  simulateEnterKeypress: () => void
}

const createMockReadlineInterface = (): MockReadlineInterface => {
  let lineListener: (() => void) | undefined
  return {
    once: (_event: string, listener: () => void) => {
      lineListener = listener
    },
    close: jest.fn<() => void>(),
    simulateEnterKeypress: () => lineListener?.(),
  }
}

type MockContextOverrides = Omit<Partial<PromptBrowserOpenContext>, 'process'> & {
  process?: Partial<PromptBrowserOpenContext['process']>
}

const createMockContext = (overrides?: MockContextOverrides): PromptBrowserOpenContext => ({
  globalInfo: () => {},
  globalWarn: () => {},
  ...overrides,
  process: {
    stdin: { isTTY: true },
    ...overrides?.process,
  },
})

beforeEach(() => {
  mockOpen.mockReset()
  mockOpen.mockResolvedValue(undefined)
})

describe('promptBrowserOpen', () => {
  it('returns the poll result when poll completes before Enter keypress', async () => {
    const mockRl = createMockReadlineInterface()
    const context = createMockContext({
      createReadlineInterface: () => mockRl,
    })

    const token = await promptBrowserOpen({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: Promise.resolve('my-token'),
    })

    expect(token).toBe('my-token')
    expect(mockRl.close).toHaveBeenCalled()
    expect(mockOpen).not.toHaveBeenCalled()
  })

  it('opens browser via open package when Enter key is pressed before poll completes', async () => {
    const mockRl = createMockReadlineInterface()
    const pollDeferred = createDeferred<string>()
    const context = createMockContext({
      createReadlineInterface: () => mockRl,
    })

    const resultPromise = promptBrowserOpen({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: pollDeferred.promise,
    })

    mockRl.simulateEnterKeypress()

    await new Promise<void>(resolve => queueMicrotask(resolve))

    expect(mockOpen).toHaveBeenCalledWith('https://example.com/auth')

    pollDeferred.resolve('token-after-enter')
    const token = await resultPromise

    expect(token).toBe('token-after-enter')
    expect(mockRl.close).toHaveBeenCalled()
  })

  it('warns and continues polling when open fails', async () => {
    const mockRl = createMockReadlineInterface()
    const pollDeferred = createDeferred<string>()
    const globalWarn = jest.fn<(msg: string) => void>()
    const globalInfo = jest.fn<(msg: string) => void>()
    mockOpen.mockRejectedValue(new Error('xdg-open not found'))
    const context = createMockContext({
      createReadlineInterface: () => mockRl,
      globalInfo,
      globalWarn,
    })

    const resultPromise = promptBrowserOpen({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: pollDeferred.promise,
    })

    mockRl.simulateEnterKeypress()

    await new Promise<void>(resolve => queueMicrotask(resolve))
    await new Promise<void>(resolve => queueMicrotask(resolve))

    expect(globalWarn).toHaveBeenCalledWith(expect.stringContaining('xdg-open not found'))
    expect(globalInfo).toHaveBeenCalledWith('Please open the URL shown above manually.')

    pollDeferred.resolve('tok')
    expect(await resultPromise).toBe('tok')
  })

  it('warns and falls back to plain poll when createReadlineInterface throws', async () => {
    const globalWarn = jest.fn<(msg: string) => void>()
    const context = createMockContext({
      createReadlineInterface: () => {
        throw new Error('setRawMode not supported')
      },
      globalWarn,
    })

    const token = await promptBrowserOpen({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: Promise.resolve('fallback-token'),
    })

    expect(token).toBe('fallback-token')
    expect(globalWarn).toHaveBeenCalledWith(expect.stringContaining('setRawMode not supported'))
    expect(mockOpen).not.toHaveBeenCalled()
  })

  it('falls back to plain poll when createReadlineInterface is not provided', async () => {
    const context = createMockContext()

    const token = await promptBrowserOpen({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: Promise.resolve('plain-token'),
    })

    expect(token).toBe('plain-token')
  })

  it('falls back to plain poll when stdin is not a TTY', async () => {
    const context = createMockContext({
      createReadlineInterface: createMockReadlineInterface,
      process: { stdin: { isTTY: false } },
    })

    const token = await promptBrowserOpen({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: Promise.resolve('plain-token'),
    })

    expect(token).toBe('plain-token')
  })

  it('shows the press-Enter message', async () => {
    const mockRl = createMockReadlineInterface()
    const globalInfo = jest.fn<(msg: string) => void>()
    const context = createMockContext({
      createReadlineInterface: () => mockRl,
      globalInfo,
    })

    await promptBrowserOpen({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: Promise.resolve('tok'),
    })

    expect(globalInfo).toHaveBeenCalledWith('Press ENTER to open the URL in your browser.')
  })

  it.each([
    ['javascript:alert(1)'],
    ['file:///etc/passwd'],
    ['not a url'],
  ])('does not open browser for non-http(s) authUrl %s', async (authUrl) => {
    const mockRl = createMockReadlineInterface()
    const pollDeferred = createDeferred<string>()
    const context = createMockContext({
      createReadlineInterface: () => mockRl,
    })

    const resultPromise = promptBrowserOpen({
      authUrl,
      context,
      pollPromise: pollDeferred.promise,
    })

    pollDeferred.resolve('tok')
    expect(await resultPromise).toBe('tok')
    expect(mockOpen).not.toHaveBeenCalled()
  })

  it('continues polling when open throws synchronously', async () => {
    const mockRl = createMockReadlineInterface()
    const pollDeferred = createDeferred<string>()
    const globalWarn = jest.fn<(msg: string) => void>()
    mockOpen.mockImplementation(() => {
      throw new Error('sync failure')
    })
    const context = createMockContext({
      createReadlineInterface: () => mockRl,
      globalWarn,
    })

    const resultPromise = promptBrowserOpen({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: pollDeferred.promise,
    })

    mockRl.simulateEnterKeypress()

    expect(globalWarn).toHaveBeenCalledWith(expect.stringContaining('sync failure'))

    pollDeferred.resolve('tok')
    expect(await resultPromise).toBe('tok')
  })

  it('cleans up when poll rejects', async () => {
    const mockRl = createMockReadlineInterface()
    const context = createMockContext({
      createReadlineInterface: () => mockRl,
    })

    await expect(promptBrowserOpen({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: Promise.reject(new Error('timeout')),
    })).rejects.toThrow('timeout')

    expect(mockRl.close).toHaveBeenCalled()
  })
})
