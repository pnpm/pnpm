import fs from 'node:fs'
import path from 'node:path'

import { beforeEach, expect, jest, test } from '@jest/globals'
import { tempDir } from '@pnpm/prepare-temp-dir'

const mockSync = jest.fn<(cmd: string, args: string[]) => { status: number, signal: NodeJS.Signals | null }>(() => ({ status: 0, signal: null }))
jest.unstable_mockModule('cross-spawn', () => ({
  default: { sync: mockSync },
}))

const { node } = await import('@pnpm/engine.runtime.commands')

const IS_WINDOWS = process.platform === 'win32'
const NODE_BIN_NAME = IS_WINDOWS ? 'node.exe' : 'node'
const PROJECT_NODE_BIN_REL = IS_WINDOWS ? 'node.exe' : 'bin/node'

beforeEach(() => {
  mockSync.mockClear()
})

test('spawns node with passed args and returns its exit code', async () => {
  const dir = tempDir()
  const pnpmHomeDir = tempDir()

  const result = await node.handler({ dir, pnpmHomeDir }, ['-v'])

  expect(mockSync).toHaveBeenCalledTimes(1)
  expect(mockSync.mock.calls[0][1]).toEqual(['-v'])
  expect(result).toEqual({ exitCode: 0 })
})

test('uses node binary from project node_modules when available', async () => {
  const dir = tempDir()
  const pnpmHomeDir = tempDir()
  const nodePkgDir = path.join(dir, 'node_modules', 'node')
  const nodeBin = path.join(nodePkgDir, PROJECT_NODE_BIN_REL)
  fs.mkdirSync(path.dirname(nodeBin), { recursive: true })
  fs.writeFileSync(path.join(nodePkgDir, 'package.json'), JSON.stringify({ name: 'node', version: '0.0.0' }))
  fs.writeFileSync(nodeBin, '')

  await node.handler({ dir, pnpmHomeDir }, [])

  expect(mockSync.mock.calls[0][0]).toBe(nodeBin)
})

test('uses node binary hoisted to workspace root when project has none', async () => {
  const workspaceDir = tempDir()
  const dir = path.join(workspaceDir, 'pkg')
  fs.mkdirSync(dir, { recursive: true })
  const pnpmHomeDir = tempDir()
  const nodePkgDir = path.join(workspaceDir, 'node_modules', 'node')
  const nodeBin = path.join(nodePkgDir, PROJECT_NODE_BIN_REL)
  fs.mkdirSync(path.dirname(nodeBin), { recursive: true })
  fs.writeFileSync(path.join(nodePkgDir, 'package.json'), JSON.stringify({ name: 'node', version: '0.0.0' }))
  fs.writeFileSync(nodeBin, '')

  await node.handler({ dir, workspaceDir, pnpmHomeDir }, [])

  expect(mockSync.mock.calls[0][0]).toBe(nodeBin)
})

test('falls back to global pnpm bin dir when no project node', async () => {
  const dir = tempDir()
  const pnpmHomeDir = tempDir()
  const globalBin = path.join(pnpmHomeDir, 'bin')
  const globalNode = path.join(globalBin, NODE_BIN_NAME)
  fs.mkdirSync(globalBin, { recursive: true })
  fs.writeFileSync(globalNode, '')

  await node.handler({ dir, pnpmHomeDir }, [])

  expect(mockSync.mock.calls[0][0]).toBe(globalNode)
})

test('honors globalBinDir when set', async () => {
  const dir = tempDir()
  const pnpmHomeDir = tempDir()
  const globalBinDir = tempDir()
  const globalNode = path.join(globalBinDir, NODE_BIN_NAME)
  fs.writeFileSync(globalNode, '')

  await node.handler({ dir, pnpmHomeDir, globalBinDir }, [])

  expect(mockSync.mock.calls[0][0]).toBe(globalNode)
})

test('falls back to PATH lookup when no node binary is resolvable', async () => {
  const dir = tempDir()
  const pnpmHomeDir = tempDir()

  await node.handler({ dir, pnpmHomeDir }, ['-v'])

  expect(mockSync.mock.calls[0][0]).toBe('node')
})

test('propagates child exit code', async () => {
  mockSync.mockReturnValueOnce({ status: 42, signal: null })
  const dir = tempDir()
  const pnpmHomeDir = tempDir()

  const result = await node.handler({ dir, pnpmHomeDir }, [])

  expect(result).toEqual({ exitCode: 42 })
})
