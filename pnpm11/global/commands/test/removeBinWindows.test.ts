import util from 'node:util'

import { beforeEach, expect, jest, test } from '@jest/globals'

interface Deferred<T> {
  promise: Promise<T>
  resolve: (value: T | PromiseLike<T>) => void
  reject: (reason?: unknown) => void
}

let deletions: Array<Deferred<void>> = []
let nextDeletion = 0
const rimraf = jest.fn<(path: string) => Promise<void>>(() => deletions[nextDeletion++].promise)

jest.unstable_mockModule('@zkochan/rimraf', () => ({ rimraf }), { virtual: true })
jest.unstable_mockModule('is-windows', () => ({ default: () => true }), { virtual: true })
// The real extension is derived from the host's COMSPEC, which is
// uppercase on Windows and would make the expected paths host-dependent.
jest.unstable_mockModule('cmd-extension', () => ({ cmdExtension: '.cmd' }), { virtual: true })

const { removeBin } = await import('../../../bins/remover/src/removeBins.js')

beforeEach(() => {
  deletions = Array.from({ length: 4 }, () => createDeferred<void>())
  nextDeletion = 0
  rimraf.mockClear()
})

test('waits for every Windows bin deletion before propagating a failure', async () => {
  const removalError = new Error('cannot remove bare bin')
  const removal = observe(removeBin('/global/bin/tool'))

  expect(rimraf.mock.calls.map(([filename]) => filename)).toStrictEqual([
    '/global/bin/tool',
    '/global/bin/tool.ps1',
    '/global/bin/tool.cmd',
    '/global/bin/tool.exe',
  ])

  deletions[0].reject(removalError)
  await nextTurn()
  expect(removal.status).toBe('pending')

  deletions[1].resolve()
  deletions[2].resolve()
  deletions[3].resolve()
  await removal.settled

  expect(removal.status).toBe('rejected')
  expect(removal.reason).toBe(removalError)
})

test('preserves every Windows bin deletion failure', async () => {
  const bareError = new Error('cannot remove bare bin')
  const executableError = new Error('cannot remove executable')
  const removal = observe(removeBin('/global/bin/tool'))

  deletions[0].reject(bareError)
  deletions[1].resolve()
  deletions[2].resolve()
  deletions[3].reject(executableError)
  await removal.settled

  expect(removal.status).toBe('rejected')
  expect(util.types.isNativeError(removal.reason)).toBe(true)
  if (!util.types.isNativeError(removal.reason) || !('errors' in removal.reason)) {
    throw new Error('Expected multiple deletion failures to be aggregated')
  }
  expect(removal.reason.errors).toStrictEqual([bareError, executableError])
})

function createDeferred<T> (): Deferred<T> {
  let resolve!: Deferred<T>['resolve']
  let reject!: Deferred<T>['reject']
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, resolve, reject }
}

function observe (promise: Promise<void>): {
  status: 'pending' | 'fulfilled' | 'rejected'
  reason?: unknown
  settled: Promise<void>
} {
  const observation: {
    status: 'pending' | 'fulfilled' | 'rejected'
    reason?: unknown
    settled: Promise<void>
  } = {
    status: 'pending',
    settled: Promise.resolve(),
  }
  observation.settled = promise.then(
    () => {
      observation.status = 'fulfilled'
    },
    (reason: unknown) => {
      observation.status = 'rejected'
      observation.reason = reason
    }
  )
  return observation
}

async function nextTurn (): Promise<void> {
  await new Promise<void>((resolve) => setImmediate(resolve))
}
