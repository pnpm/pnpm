import { detectIfCurrentPkgIsExecutable } from '@pnpm/cli-meta'
import which from 'which'
import { getNodeExecPath } from '../lib/nodeExecPath.js'

jest.mock('which', () => jest.fn())
jest.mock('@pnpm/cli-meta', () => ({
  detectIfCurrentPkgIsExecutable: jest.fn(),
}))

const whichMock = jest.mocked(which)
const detectMock = jest.mocked(detectIfCurrentPkgIsExecutable)

afterEach(() => {
  whichMock.mockReset()
  detectMock.mockReset()
})

test('returns undefined when node is not on PATH and pnpm is running as @pnpm/exe', async () => {
  const enoent: NodeJS.ErrnoException = Object.assign(new Error('not found: node'), { code: 'ENOENT' })
  whichMock.mockRejectedValue(enoent)
  detectMock.mockReturnValue(true)

  await expect(getNodeExecPath()).resolves.toBeUndefined()
})

test('falls back to process.execPath when node is not on PATH and pnpm is running under a real Node.js', async () => {
  const enoent: NodeJS.ErrnoException = Object.assign(new Error('not found: node'), { code: 'ENOENT' })
  whichMock.mockRejectedValue(enoent)
  detectMock.mockReturnValue(false)
  const savedNode = process.env.NODE
  delete process.env.NODE
  try {
    await expect(getNodeExecPath()).resolves.toBe(process.execPath)
  } finally {
    if (savedNode != null) process.env.NODE = savedNode
  }
})
