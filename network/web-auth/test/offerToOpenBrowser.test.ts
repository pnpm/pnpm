import { jest } from '@jest/globals'
import {
  offerToOpenBrowser,
  type OfferToOpenBrowserContext,
  type OfferToOpenBrowserExecFile,
  type OfferToOpenBrowserReadlineInterface,
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

interface MockReadlineInterface extends OfferToOpenBrowserReadlineInterface {
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

type MockContextOverrides = Omit<Partial<OfferToOpenBrowserContext>, 'process'> & {
  process?: Partial<OfferToOpenBrowserContext['process']>
}

const createMockContext = (overrides?: MockContextOverrides): OfferToOpenBrowserContext => ({
  globalInfo: () => {},
  globalWarn: () => {},
  ...overrides,
  process: {
    platform: 'linux',
    stdin: { isTTY: true },
    ...overrides?.process,
  },
})

describe('offerToOpenBrowser', () => {
  it('returns the poll result when poll completes before Enter keypress', async () => {
    const mockRl = createMockReadlineInterface()
    const execFile = jest.fn<OfferToOpenBrowserExecFile>()
    const context = createMockContext({
      createReadlineInterface: () => mockRl,
      execFile,
    })

    const token = await offerToOpenBrowser({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: Promise.resolve('my-token'),
    })

    expect(token).toBe('my-token')
    expect(mockRl.close).toHaveBeenCalled()
    expect(execFile).not.toHaveBeenCalled()
  })

  it('opens browser via execFile when Enter key is pressed before poll completes', async () => {
    const mockRl = createMockReadlineInterface()
    const pollDeferred = createDeferred<string>()
    const execFile = jest.fn<OfferToOpenBrowserExecFile>((_file, _args, cb) => {
      cb(null)
    })
    const context = createMockContext({
      createReadlineInterface: () => mockRl,
      execFile,
    })

    const resultPromise = offerToOpenBrowser({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: pollDeferred.promise,
    })

    mockRl.simulateEnterKeypress()

    await new Promise<void>(resolve => queueMicrotask(resolve))

    expect(execFile).toHaveBeenCalledWith('xdg-open', ['https://example.com/auth'], expect.any(Function))

    pollDeferred.resolve('token-after-enter')
    const token = await resultPromise

    expect(token).toBe('token-after-enter')
    expect(mockRl.close).toHaveBeenCalled()
  })

  it('uses "open" on darwin', async () => {
    const mockRl = createMockReadlineInterface()
    const pollDeferred = createDeferred<string>()
    const execFile = jest.fn<OfferToOpenBrowserExecFile>((_file, _args, cb) => {
      cb(null)
    })
    const context = createMockContext({
      createReadlineInterface: () => mockRl,
      execFile,
      process: { platform: 'darwin' },
    })

    const resultPromise = offerToOpenBrowser({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: pollDeferred.promise,
    })

    mockRl.simulateEnterKeypress()
    await new Promise<void>(resolve => queueMicrotask(resolve))

    expect(execFile).toHaveBeenCalledWith('open', ['https://example.com/auth'], expect.any(Function))

    pollDeferred.resolve('tok')
    await resultPromise
  })

  it('uses "cmd /c start" on win32', async () => {
    const mockRl = createMockReadlineInterface()
    const pollDeferred = createDeferred<string>()
    const execFile = jest.fn<OfferToOpenBrowserExecFile>((_file, _args, cb) => {
      cb(null)
    })
    const context = createMockContext({
      createReadlineInterface: () => mockRl,
      execFile,
      process: { platform: 'win32' },
    })

    const resultPromise = offerToOpenBrowser({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: pollDeferred.promise,
    })

    mockRl.simulateEnterKeypress()
    await new Promise<void>(resolve => queueMicrotask(resolve))

    expect(execFile).toHaveBeenCalledWith('cmd', ['/c', 'start', '', 'https://example.com/auth'], expect.any(Function))

    pollDeferred.resolve('tok')
    await resultPromise
  })

  it('skips browser prompt on unsupported platform', async () => {
    const execFile = jest.fn<OfferToOpenBrowserExecFile>()
    const context = createMockContext({
      createReadlineInterface: createMockReadlineInterface,
      execFile,
      process: { platform: 'freebsd' },
    })

    const token = await offerToOpenBrowser({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: Promise.resolve('plain-token'),
    })

    expect(token).toBe('plain-token')
    expect(execFile).not.toHaveBeenCalled()
  })

  it('skips browser prompt when platform is undefined', async () => {
    const execFile = jest.fn<OfferToOpenBrowserExecFile>()
    const context = createMockContext({
      createReadlineInterface: createMockReadlineInterface,
      execFile,
      process: { platform: undefined },
    })

    const token = await offerToOpenBrowser({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: Promise.resolve('plain-token'),
    })

    expect(token).toBe('plain-token')
    expect(execFile).not.toHaveBeenCalled()
  })

  it('warns and continues polling when execFile fails', async () => {
    const mockRl = createMockReadlineInterface()
    const pollDeferred = createDeferred<string>()
    const globalWarn = jest.fn<(msg: string) => void>()
    const globalInfo = jest.fn<(msg: string) => void>()
    const execFile = jest.fn<OfferToOpenBrowserExecFile>((_file, _args, cb) => {
      cb(new Error('xdg-open not found'))
    })
    const context = createMockContext({
      createReadlineInterface: () => mockRl,
      execFile,
      globalInfo,
      globalWarn,
    })

    const resultPromise = offerToOpenBrowser({
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
    const execFile = jest.fn<OfferToOpenBrowserExecFile>()
    const context = createMockContext({
      createReadlineInterface: () => {
        throw new Error('setRawMode not supported')
      },
      execFile,
      globalWarn,
    })

    const token = await offerToOpenBrowser({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: Promise.resolve('fallback-token'),
    })

    expect(token).toBe('fallback-token')
    expect(globalWarn).toHaveBeenCalledWith(expect.stringContaining('setRawMode not supported'))
    expect(execFile).not.toHaveBeenCalled()
  })

  it('falls back to plain poll when createReadlineInterface is not provided', async () => {
    const context = createMockContext({
      execFile: jest.fn<OfferToOpenBrowserExecFile>(),
    })

    const token = await offerToOpenBrowser({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: Promise.resolve('plain-token'),
    })

    expect(token).toBe('plain-token')
  })

  it('falls back to plain poll when execFile is not provided', async () => {
    const context = createMockContext({
      createReadlineInterface: createMockReadlineInterface,
    })

    const token = await offerToOpenBrowser({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: Promise.resolve('plain-token'),
    })

    expect(token).toBe('plain-token')
  })

  it('falls back to plain poll when stdin is not a TTY', async () => {
    const context = createMockContext({
      createReadlineInterface: createMockReadlineInterface,
      execFile: jest.fn<OfferToOpenBrowserExecFile>(),
      process: { stdin: { isTTY: false } },
    })

    const token = await offerToOpenBrowser({
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
      execFile: jest.fn<OfferToOpenBrowserExecFile>(),
      globalInfo,
    })

    await offerToOpenBrowser({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: Promise.resolve('tok'),
    })

    expect(globalInfo).toHaveBeenCalledWith('Press ENTER to open the URL in your browser.')
  })

  it('cleans up when poll rejects', async () => {
    const mockRl = createMockReadlineInterface()
    const context = createMockContext({
      createReadlineInterface: () => mockRl,
      execFile: jest.fn<OfferToOpenBrowserExecFile>(),
    })

    await expect(offerToOpenBrowser({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: Promise.reject(new Error('timeout')),
    })).rejects.toThrow('timeout')

    expect(mockRl.close).toHaveBeenCalled()
  })
})
