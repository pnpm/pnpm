import { afterEach, expect, jest, test } from '@jest/globals'

const execaSync = jest.fn()
jest.unstable_mockModule('execa', () => ({
  sync: execaSync,
}))

const detectIfCurrentPkgIsExecutable = jest.fn<() => boolean>(() => false)
jest.unstable_mockModule('@pnpm/cli.meta', () => ({
  detectIfCurrentPkgIsExecutable,
}))

const { runPnpmCli } = await import('@pnpm/exec.pnpm-cli-runner')

const originalArgv = [...process.argv]

afterEach(() => {
  execaSync.mockClear()
  detectIfCurrentPkgIsExecutable.mockReturnValue(false)
  process.argv = [...originalArgv]
})

test('the @pnpm/exe build re-runs itself', () => {
  detectIfCurrentPkgIsExecutable.mockReturnValue(true)

  runPnpmCli(['add', 'express'], { cwd: '/test' })

  expect(execaSync).toHaveBeenCalledWith(process.execPath, ['add', 'express'], {
    cwd: '/test',
    stdio: 'inherit',
  })
})

test.each([
  '/path/to/pnpm.mjs',
  '/path/to/pnpm.cjs',
  '/path/to/dist/pnpm.js',
  '/path/to/node_modules/.bin/pnpm',
])('the entry script %s is re-run with Node.js', (entryScript) => {
  process.argv[1] = entryScript

  runPnpmCli(['add', 'express'], { cwd: '/test' })

  expect(execaSync).toHaveBeenCalledWith(process.execPath, [entryScript, 'add', 'express'], {
    cwd: '/test',
    stdio: 'inherit',
  })
})

test('the reporter is passed to the spawned CLI', () => {
  process.argv[1] = '/path/to/pnpm.mjs'

  runPnpmCli(['install'], { cwd: '/test', reporter: 'silent' })

  expect(execaSync).toHaveBeenCalledWith(process.execPath, ['/path/to/pnpm.mjs', 'install', '--reporter=silent'], {
    cwd: '/test',
    stdio: 'inherit',
  })
})

test('a process without an entry script falls back to the pnpm on PATH', () => {
  process.argv = [process.argv[0]]

  runPnpmCli(['add', 'express'], { cwd: '/test' })

  expect(execaSync).toHaveBeenCalledWith('pnpm', ['add', 'express'], {
    cwd: '/test',
    stdio: 'inherit',
  })
})
