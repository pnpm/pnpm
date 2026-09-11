import { afterEach, describe, expect, jest, test } from '@jest/globals'

const mockSync = jest.fn()

jest.unstable_mockModule('execa', () => ({
  sync: mockSync,
}))

const { runPnpmCli } = await import('../src/index.js')

describe('runPnpmCli', () => {
  const originalExecPath = process.execPath
  const originalArgv = [...process.argv]

  afterEach(() => {
    mockSync.mockClear()
    Object.defineProperty(process, 'execPath', { value: originalExecPath, writable: true })
    process.argv = [...originalArgv]
  })

  test('runs process.execPath directly when execPath basename is pnpm', () => {
    Object.defineProperty(process, 'execPath', { value: '/usr/local/bin/pnpm', writable: true })
    runPnpmCli(['add', 'express'], { cwd: '/test' })
    expect(mockSync).toHaveBeenCalledWith('/usr/local/bin/pnpm', ['add', 'express'], {
      cwd: '/test',
      stdio: 'inherit',
    })
  })

  test('runs node with process.argv[1] when execPath is node and process.argv[1] is set', () => {
    Object.defineProperty(process, 'execPath', { value: '/usr/bin/node', writable: true })
    process.argv[1] = '/path/to/pnpm.cjs'
    runPnpmCli(['add', 'express'], { cwd: '/test' })
    expect(mockSync).toHaveBeenCalledWith('/usr/bin/node', ['/path/to/pnpm.cjs', 'add', 'express'], {
      cwd: '/test',
      stdio: 'inherit',
    })
  })

  test('runs node with process.argv[1] and reporter flag when specified', () => {
    Object.defineProperty(process, 'execPath', { value: '/usr/bin/node', writable: true })
    process.argv[1] = '/path/to/pnpm.js'
    runPnpmCli(['install'], { cwd: '/test', reporter: 'silent' })
    expect(mockSync).toHaveBeenCalledWith('/usr/bin/node', ['/path/to/pnpm.js', 'install', '--reporter=silent'], {
      cwd: '/test',
      stdio: 'inherit',
    })
  })

  test('falls back to "pnpm" binary when process.argv[1] is empty or not a pnpm script', () => {
    Object.defineProperty(process, 'execPath', { value: '/usr/bin/node', writable: true })
    process.argv[1] = ''
    runPnpmCli(['add', 'express'], { cwd: '/test' })
    expect(mockSync).toHaveBeenCalledWith('pnpm', ['add', 'express'], {
      cwd: '/test',
      stdio: 'inherit',
    })

    mockSync.mockClear()
    process.argv[1] = '/node_modules/jest/bin/jest.js'
    runPnpmCli(['add', 'express'], { cwd: '/test' })
    expect(mockSync).toHaveBeenCalledWith('pnpm', ['add', 'express'], {
      cwd: '/test',
      stdio: 'inherit',
    })
  })
})
