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
  '/path/to/pnpm/bin/pnpm.mjs',
  '/path/to/pnpm/bin/pnpm.cjs',
  '/path/to/pnpm/dist/pnpm.mjs',
  '/path/to/node_modules/.bin/pnpm',
  '/path/to/node_modules/.bin/pn',
])('the entry script %s is re-run with Node.js', (entryScript) => {
  process.argv[1] = entryScript

  runPnpmCli(['add', 'express'], { cwd: '/test' })

  expect(execaSync).toHaveBeenCalledWith(process.execPath, [entryScript, 'add', 'express'], {
    cwd: '/test',
    stdio: 'inherit',
  })
})

test('the reporter is passed to the spawned CLI', () => {
  process.argv[1] = '/path/to/pnpm/bin/pnpm.mjs'

  runPnpmCli(['install'], { cwd: '/test', reporter: 'silent' })

  expect(execaSync).toHaveBeenCalledWith(process.execPath, ['/path/to/pnpm/bin/pnpm.mjs', 'install', '--reporter=silent'], {
    cwd: '/test',
    stdio: 'inherit',
  })
})

test.each([
  // A host that merely imports pnpm's packages, such as the Jest runs of the
  // commands that call runPnpmCli.
  '/path/to/node_modules/jest/bin/jest.js',
  // Re-running pnpx would prepend `dlx` to the command.
  '/path/to/pnpm/bin/pnpx.mjs',
])('the entry script %s falls back to the pnpm on PATH', (entryScript) => {
  process.argv[1] = entryScript

  runPnpmCli(['add', 'express'], { cwd: '/test' })

  expect(execaSync).toHaveBeenCalledWith('pnpm', ['add', 'express'], {
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
