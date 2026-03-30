import { PassThrough } from 'node:stream'

import { jest } from '@jest/globals'
import {
  offerToOpenBrowser,
  type OfferToOpenBrowserContext,
  type OfferToOpenBrowserExecFile,
  type OfferToOpenBrowserReadline,
  type OfferToOpenBrowserReadlineInterface,
  type OfferToOpenBrowserStdin,
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
  simulateEnter: () => void
}

function createMockReadlineInterface (): MockReadlineInterface {
  let lineListener: (() => void) | undefined
  return {
    once: (_event: string, listener: () => void) => {
      lineListener = listener
    },
    close: jest.fn<() => void>(),
    simulateEnter: () => lineListener?.(),
  }
}

function createMockStdin (isTTY: boolean = true): OfferToOpenBrowserStdin {
  return Object.assign(new PassThrough(), { isTTY })
}

function createMockContext (overrides?: Partial<OfferToOpenBrowserContext>): OfferToOpenBrowserContext {
  return {
    globalInfo: () => {},
    globalWarn: () => {},
    process: {
      platform: 'linux',
      stdin: createMockStdin(),
    },
    ...overrides,
  }
}

describe('offerToOpenBrowser', () => {
  it('returns the poll result when poll completes before Enter', async () => {
    const mockRl = createMockReadlineInterface()
    const execFile = jest.fn<OfferToOpenBrowserExecFile>()
    const readline: OfferToOpenBrowserReadline = {
      createInterface: () => mockRl,
    }

    const context = createMockContext({ execFile, readline })

    const token = await offerToOpenBrowser({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: Promise.resolve('my-token'),
    })

    expect(token).toBe('my-token')
    expect(mockRl.close).toHaveBeenCalled()
    expect(execFile).not.toHaveBeenCalled()
  })

  it('opens browser via execFile when Enter is pressed before poll completes', async () => {
    const mockRl = createMockReadlineInterface()
    const pollDeferred = createDeferred<string>()
    const execFile = jest.fn<OfferToOpenBrowserExecFile>((_file, _args, cb) => {
      cb(null)
    })
    const readline: OfferToOpenBrowserReadline = {
      createInterface: () => mockRl,
    }

    const context = createMockContext({ execFile, readline })

    const resultPromise = offerToOpenBrowser({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: pollDeferred.promise,
    })

    mockRl.simulateEnter()

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
    const readline: OfferToOpenBrowserReadline = {
      createInterface: () => mockRl,
    }

    const context = createMockContext({
      execFile,
      readline,
      process: { platform: 'darwin', stdin: createMockStdin() },
    })

    const resultPromise = offerToOpenBrowser({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: pollDeferred.promise,
    })

    mockRl.simulateEnter()
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
    const readline: OfferToOpenBrowserReadline = {
      createInterface: () => mockRl,
    }

    const context = createMockContext({
      execFile,
      readline,
      process: { platform: 'win32', stdin: createMockStdin() },
    })

    const resultPromise = offerToOpenBrowser({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: pollDeferred.promise,
    })

    mockRl.simulateEnter()
    await new Promise<void>(resolve => queueMicrotask(resolve))

    expect(execFile).toHaveBeenCalledWith('cmd', ['/c', 'start', '', 'https://example.com/auth'], expect.any(Function))

    pollDeferred.resolve('tok')
    await resultPromise
  })

  it('skips browser prompt on unsupported platform', async () => {
    const execFile = jest.fn<OfferToOpenBrowserExecFile>()
    const readline: OfferToOpenBrowserReadline = {
      createInterface: jest.fn<OfferToOpenBrowserReadline['createInterface']>(),
    }

    const context = createMockContext({
      execFile,
      readline,
      process: { platform: 'freebsd', stdin: createMockStdin() },
    })

    const token = await offerToOpenBrowser({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: Promise.resolve('plain-token'),
    })

    expect(token).toBe('plain-token')
    expect(readline.createInterface).not.toHaveBeenCalled()
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
    const readline: OfferToOpenBrowserReadline = {
      createInterface: () => mockRl,
    }

    const context = createMockContext({ execFile, globalInfo, globalWarn, readline })

    const resultPromise = offerToOpenBrowser({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: pollDeferred.promise,
    })

    mockRl.simulateEnter()

    await new Promise<void>(resolve => queueMicrotask(resolve))
    await new Promise<void>(resolve => queueMicrotask(resolve))

    expect(globalWarn).toHaveBeenCalledWith(expect.stringContaining('xdg-open not found'))
    expect(globalInfo).toHaveBeenCalledWith('Please open the URL shown above manually.')

    pollDeferred.resolve('tok')
    expect(await resultPromise).toBe('tok')
  })

  it('warns and falls back to plain poll when createInterface throws', async () => {
    const globalWarn = jest.fn<(msg: string) => void>()
    const execFile = jest.fn<OfferToOpenBrowserExecFile>()
    const readline: OfferToOpenBrowserReadline = {
      createInterface: () => {
        throw new Error('setRawMode not supported')
      },
    }

    const context = createMockContext({ execFile, globalWarn, readline })

    const token = await offerToOpenBrowser({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: Promise.resolve('fallback-token'),
    })

    expect(token).toBe('fallback-token')
    expect(globalWarn).toHaveBeenCalledWith(expect.stringContaining('setRawMode not supported'))
    expect(execFile).not.toHaveBeenCalled()
  })

  it('falls back to plain poll when readline is not provided', async () => {
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
    const readline: OfferToOpenBrowserReadline = {
      createInterface: jest.fn<OfferToOpenBrowserReadline['createInterface']>(),
    }
    const context = createMockContext({ readline })

    const token = await offerToOpenBrowser({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: Promise.resolve('plain-token'),
    })

    expect(token).toBe('plain-token')
    expect(readline.createInterface).not.toHaveBeenCalled()
  })

  it('falls back to plain poll when stdin is not a TTY', async () => {
    const execFile = jest.fn<OfferToOpenBrowserExecFile>()
    const readline: OfferToOpenBrowserReadline = {
      createInterface: jest.fn<OfferToOpenBrowserReadline['createInterface']>(),
    }
    const context = createMockContext({
      execFile,
      readline,
      process: { platform: 'linux', stdin: createMockStdin(false) },
    })

    const token = await offerToOpenBrowser({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: Promise.resolve('plain-token'),
    })

    expect(token).toBe('plain-token')
    expect(readline.createInterface).not.toHaveBeenCalled()
  })

  it('shows the press-Enter message', async () => {
    const mockRl = createMockReadlineInterface()
    const globalInfo = jest.fn<(msg: string) => void>()
    const context = createMockContext({
      execFile: jest.fn<OfferToOpenBrowserExecFile>(),
      globalInfo,
      readline: { createInterface: () => mockRl },
    })

    await offerToOpenBrowser({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: Promise.resolve('tok'),
    })

    expect(globalInfo).toHaveBeenCalledWith('Press ENTER to open in browser...')
  })

  it('cleans up when poll rejects', async () => {
    const mockRl = createMockReadlineInterface()
    const context = createMockContext({
      execFile: jest.fn<OfferToOpenBrowserExecFile>(),
      readline: { createInterface: () => mockRl },
    })

    await expect(offerToOpenBrowser({
      authUrl: 'https://example.com/auth',
      context,
      pollPromise: Promise.reject(new Error('timeout')),
    })).rejects.toThrow('timeout')

    expect(mockRl.close).toHaveBeenCalled()
  })
})
