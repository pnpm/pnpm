import { expect, jest, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'

jest.unstable_mockModule('@pnpm/deps.status', () => ({
  checkDepsStatus: jest.fn(async () => ({ upToDate: false, workspaceState: undefined })),
}))

const mockRunPnpmCli = jest.fn()
jest.unstable_mockModule('@pnpm/exec.pnpm-cli-runner', () => ({
  runPnpmCli: mockRunPnpmCli,
}))

const { runDepsStatusCheck } = await import('../src/runDepsStatusCheck.js')

const baseOpts = {
  dir: process.cwd(),
  verifyDepsBeforeRun: 'install' as const,
  pnpmfile: [],
  linkWorkspacePackages: false,
  rootProjectManifestDir: process.cwd(),
}

test('includes --filter args in the install command when filter is set', async () => {
  prepare({ name: 'root', private: true })

  mockRunPnpmCli.mockReturnValue(undefined)

  await runDepsStatusCheck({
    ...baseOpts,
    filter: ['foo', 'bar'],
  })

  expect(mockRunPnpmCli).toHaveBeenCalledWith(
    expect.arrayContaining(['install', '--filter=foo', '--filter=bar']),
    expect.any(Object)
  )
})

test('does not add --filter args when filter is empty', async () => {
  prepare({ name: 'root', private: true })

  mockRunPnpmCli.mockReturnValue(undefined)

  await runDepsStatusCheck({
    ...baseOpts,
    filter: [],
  })

  const calledCommand = mockRunPnpmCli.mock.calls[0]?.[0] as string[]
  expect(calledCommand).not.toContain('--filter=foo')
  expect(calledCommand[0]).toBe('install')
})

test('does not add --filter args when filter is undefined', async () => {
  prepare({ name: 'root', private: true })

  mockRunPnpmCli.mockReturnValue(undefined)

  await runDepsStatusCheck(baseOpts)

  const calledCommand = mockRunPnpmCli.mock.calls[0]?.[0] as string[]
  expect(calledCommand).not.toEqual(expect.arrayContaining([expect.stringContaining('--filter=')]))
  expect(calledCommand[0]).toBe('install')
})
