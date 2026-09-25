import { afterEach, expect, jest, test } from '@jest/globals'

const execaSync = jest.fn()
jest.unstable_mockModule('execa', () => ({
  sync: execaSync,
}))

const resolvePnpmSelfCommand = jest.fn<() => string[]>()
jest.unstable_mockModule('@pnpm/cli.meta', () => ({
  resolvePnpmSelfCommand,
}))

const { runPnpmCli } = await import('@pnpm/exec.pnpm-cli-runner')

afterEach(() => {
  execaSync.mockClear()
  resolvePnpmSelfCommand.mockReset()
})

test.each([
  [['/usr/bin/node', '/path/to/pnpm/bin/pnpm.mjs']],
  [['/path/to/pnpm']],
  [['pnpm']],
])('the command is appended to the self command %j', (selfCommand) => {
  resolvePnpmSelfCommand.mockReturnValue(selfCommand)

  runPnpmCli(['add', 'express'], { cwd: '/test' })

  const [executable, ...selfArgs] = selfCommand
  expect(execaSync).toHaveBeenCalledWith(executable, [...selfArgs, 'add', 'express'], {
    cwd: '/test',
    stdio: 'inherit',
  })
})

test('the reporter is passed to the spawned CLI', () => {
  resolvePnpmSelfCommand.mockReturnValue(['/usr/bin/node', '/path/to/pnpm/bin/pnpm.mjs'])

  runPnpmCli(['install'], { cwd: '/test', reporter: 'silent' })

  expect(execaSync).toHaveBeenCalledWith('/usr/bin/node', ['/path/to/pnpm/bin/pnpm.mjs', 'install', '--reporter=silent'], {
    cwd: '/test',
    stdio: 'inherit',
  })
})
