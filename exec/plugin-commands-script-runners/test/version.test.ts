import { prepareEmpty } from '@pnpm/prepare'
import { jest } from '@jest/globals'
import { type ProjectsGraph, type ProjectRootDir } from '@pnpm/types'
import { DEFAULT_OPTS } from './utils/index.js'

jest.unstable_mockModule('@pnpm/run-npm', () => ({
  runNpm: jest.fn(() => ({ status: 0 })),
}))

const { runNpm } = await import('@pnpm/run-npm')
const { version } = await import('@pnpm/plugin-commands-script-runners')

beforeEach(() => {
  jest.clearAllMocks()
})

test('version should invoke runNpm with version params and dir', async () => {
  prepareEmpty()

  const cwd = process.cwd() as ProjectRootDir

  await version.handler({
    ...DEFAULT_OPTS,
    dir: cwd,
    configDir: cwd,
    extraEnv: { FOO: 'bar' },
    selectedProjectsGraph: {
      [cwd]: {
        dependencies: [],
        package: {
          manifest: {
            name: 'foo',
            version: '1.0.0',
          },
          rootDir: cwd,
          rootDirRealPath: cwd as any, // eslint-disable-line @typescript-eslint/no-explicit-any
        },
      },
    } as unknown as ProjectsGraph,
  }, ['minor'])

  expect(runNpm).toHaveBeenCalledWith(undefined, ['version', 'minor'], expect.objectContaining({
    cwd,
    env: { FOO: 'bar' },
  }))
})
