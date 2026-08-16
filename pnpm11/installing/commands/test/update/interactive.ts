import path from 'node:path'

import { expect, jest, test } from '@jest/globals'
import type { LockfileObject } from '@pnpm/lockfile.types'
import { prepare, preparePackages } from '@pnpm/prepare'
import { addDistTag, REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
import { filterProjectsBySelectorObjectsFromDir } from '@pnpm/workspace.projects-filter'
import chalk from 'chalk'
import { readYamlFileSync } from 'read-yaml-file'

jest.unstable_mockModule('@inquirer/prompts', () => {
  class Separator {
    separator: string
    readonly type = 'separator' as const
    constructor (separator: string) {
      this.separator = separator
    }
  }
  return {
    Separator,
    checkbox: jest.fn(),
    confirm: jest.fn(),
    input: jest.fn(),
    password: jest.fn(),
    select: jest.fn(),
  }
})
const { checkbox, Separator } = await import('@inquirer/prompts')
const { add, install, update } = await import('@pnpm/installing.commands')

const mockCheckbox = jest.mocked(checkbox)

const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}`

const DEFAULT_OPTIONS = {
  argv: {
    original: [],
  },
  bail: false,
  bin: 'node_modules/.bin',
  excludeLinksFromLockfile: false,
  extraEnv: {},
  cliOptions: {},
  deployAllFiles: false,
  include: {
    dependencies: true,
    devDependencies: true,
    optionalDependencies: true,
  },
  lock: true,
  pnpmfile: ['.pnpmfile.cjs'],
  pnpmHomeDir: '',
  preferWorkspacePackages: true,
  configByUri: {},
  registriesByScope: {
    default: REGISTRY_URL,
  },
  rootProjectManifestDir: '',
  sort: true,
  userConfig: {},
  workspaceConcurrency: 1,
  virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
}

test('global interactive update handles an empty global directory', async () => {
  const globalDir = path.resolve('empty-global')

  await expect(update.handler({
    ...DEFAULT_OPTIONS,
    bin: path.resolve('bin'),
    dir: process.cwd(),
    global: true,
    globalPkgDir: globalDir,
    interactive: true,
  } as any)).resolves.toBe('No global packages found') // eslint-disable-line @typescript-eslint/no-explicit-any
  expect(mockCheckbox).not.toHaveBeenCalled()
})

test('global interactive update reads current versions from the global virtual store', async () => {
  prepare()
  const globalDir = path.resolve('global')
  const storeDir = path.resolve('pnpm-store')
  const options = {
    ...DEFAULT_OPTIONS,
    allowBuilds: {},
    bin: path.resolve('bin'),
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    enableGlobalVirtualStore: true,
    global: true,
    globalPkgDir: globalDir,
    pnpmHomeDir: '',
    storeDir,
  }

  await add.handler(options as any, ['@pnpm.e2e/multi-version-a@1.0.0']) // eslint-disable-line @typescript-eslint/no-explicit-any
  await addDistTag({ package: '@pnpm.e2e/multi-version-a', version: '2.1.0', distTag: 'latest' })
  mockCheckbox.mockResolvedValue([])

  await update.handler({
    ...options,
    interactive: true,
    latest: true,
  } as any) // eslint-disable-line @typescript-eslint/no-explicit-any

  expect(mockCheckbox).toHaveBeenCalledWith(expect.objectContaining({
    choices: [{
      name: '@pnpm.e2e/multi-version-a 1.0.0 → 2.1.0',
      value: expect.any(String),
    }],
  }))
})

test('global interactive update offers a matching group even when the requested package is current', async () => {
  prepare()
  const options = globalOptions()

  await addDistTag({ package: '@pnpm.e2e/multi-version-b', version: '3.1.0', distTag: 'latest' })
  // One comma-joined param installs both packages into a single group.
  await add.handler(options as any, ['@pnpm.e2e/multi-version-a@1.0.0,@pnpm.e2e/multi-version-b@3.1.0']) // eslint-disable-line @typescript-eslint/no-explicit-any
  await addDistTag({ package: '@pnpm.e2e/multi-version-a', version: '2.1.0', distTag: 'latest' })
  mockCheckbox.mockClear()
  mockCheckbox.mockResolvedValue([])

  await update.handler({
    ...options,
    interactive: true,
    latest: true,
  } as any, ['@pnpm.e2e/multi-version-b']) // eslint-disable-line @typescript-eslint/no-explicit-any

  expect(mockCheckbox).toHaveBeenCalledWith(expect.objectContaining({
    choices: [{
      name: '@pnpm.e2e/multi-version-a 1.0.0 → 2.1.0',
      value: expect.any(String),
    }],
  }))
})

test('global interactive update reports when no group has the requested package', async () => {
  prepare()
  const options = globalOptions()

  await add.handler(options as any, ['@pnpm.e2e/multi-version-a@1.0.0']) // eslint-disable-line @typescript-eslint/no-explicit-any
  mockCheckbox.mockClear()

  await expect(update.handler({
    ...options,
    interactive: true,
    latest: true,
  } as any, ['@pnpm.e2e/multi-version-c'])).resolves.toBe('No matching global packages found') // eslint-disable-line @typescript-eslint/no-explicit-any
  expect(mockCheckbox).not.toHaveBeenCalled()
})

// pacquet reads only the wanted lockfile, which its global install writes
// whatever `lockfile` is set to; here the current lockfile carries the
// versions instead. Both stacks must keep reporting them.
test('global interactive update reads current versions when the lockfile setting is off', async () => {
  prepare()
  const options = { ...globalOptions(), useLockfile: false }

  await add.handler(options as any, ['@pnpm.e2e/multi-version-a@1.0.0']) // eslint-disable-line @typescript-eslint/no-explicit-any
  await addDistTag({ package: '@pnpm.e2e/multi-version-a', version: '2.1.0', distTag: 'latest' })
  mockCheckbox.mockClear()
  mockCheckbox.mockResolvedValue([])

  await update.handler({
    ...options,
    interactive: true,
    latest: true,
  } as any) // eslint-disable-line @typescript-eslint/no-explicit-any

  expect(mockCheckbox).toHaveBeenCalledWith(expect.objectContaining({
    choices: [{
      name: '@pnpm.e2e/multi-version-a 1.0.0 → 2.1.0',
      value: expect.any(String),
    }],
  }))
})

test('global interactive update does not match a group on an inherited Object key', async () => {
  prepare()
  const options = globalOptions()

  await add.handler(options as any, ['@pnpm.e2e/multi-version-a@1.0.0']) // eslint-disable-line @typescript-eslint/no-explicit-any
  mockCheckbox.mockClear()

  await expect(update.handler({
    ...options,
    interactive: true,
    latest: true,
  } as any, ['constructor'])).resolves.toBe('No matching global packages found') // eslint-disable-line @typescript-eslint/no-explicit-any
  expect(mockCheckbox).not.toHaveBeenCalled()
})

test('global interactive update leaves without an error when the prompt is canceled', async () => {
  prepare()
  const options = globalOptions()

  await add.handler(options as any, ['@pnpm.e2e/multi-version-a@1.0.0']) // eslint-disable-line @typescript-eslint/no-explicit-any
  await addDistTag({ package: '@pnpm.e2e/multi-version-a', version: '2.1.0', distTag: 'latest' })
  const canceled = new Error('User force closed the prompt')
  canceled.name = 'ExitPromptError'
  mockCheckbox.mockClear()
  mockCheckbox.mockRejectedValue(canceled)
  // `process.exit()` never returns in production, so the spy throws to stop
  // the handler where the real call would.
  const exited = new Error('process.exit')
  const exitSpy = jest.spyOn(process, 'exit').mockImplementation(() => {
    throw exited
  })

  try {
    await expect(update.handler({
      ...options,
      interactive: true,
      latest: true,
    } as any)).rejects.toBe(exited) // eslint-disable-line @typescript-eslint/no-explicit-any
    expect(exitSpy).toHaveBeenCalledWith(0)
  } finally {
    exitSpy.mockRestore()
    mockCheckbox.mockReset()
  }
})

function globalOptions (): Record<string, unknown> {
  return {
    ...DEFAULT_OPTIONS,
    allowBuilds: {},
    bin: path.resolve('bin'),
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    global: true,
    globalPkgDir: path.resolve('global'),
    pnpmHomeDir: '',
    storeDir: path.resolve('pnpm-store'),
  }
}

test('interactively update', async () => {
  const project = prepare({
    dependencies: {
      // has 1.0.0 and 1.0.1 that satisfy this range
      '@pnpm.e2e/multi-version-a': '^1.0.0',
      // only 2.0.0 satisfies this range
      '@pnpm.e2e/multi-version-b': '^2.0.0',
      // has several versions that satisfy ^3.0.0
      '@pnpm.e2e/multi-version-c': '^3.0.0',
    },
  })

  const storeDir = path.resolve('pnpm-store')

  await Promise.all([
    addDistTag({ package: '@pnpm.e2e/multi-version-a', version: '2.1.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/multi-version-c', version: '4.0.0', distTag: 'latest' }),
  ])

  await add.handler(
    {
      ...DEFAULT_OPTIONS,
      cacheDir: path.resolve('cache'),
      dir: process.cwd(),
      linkWorkspacePackages: true,
      save: false,
      storeDir,
    },
    ['@pnpm.e2e/multi-version-a@1.0.0', '@pnpm.e2e/multi-version-b@2.0.0', '@pnpm.e2e/multi-version-c@3.0.0']
  )

  mockCheckbox.mockResolvedValue(['@pnpm.e2e/multi-version-a'])

  mockCheckbox.mockClear()
  // Update to compatible versions
  await update.handler({
    ...DEFAULT_OPTIONS,
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    interactive: true,
    linkWorkspacePackages: true,
    storeDir,
  })

  // eslint-disable-next-line
  const callArgs = mockCheckbox.mock.calls[0][0] as any
  const flatChoices = callArgs.choices

  expect(flatChoices).toStrictEqual([
    new Separator(chalk.bold('── dependencies ──')),
    new Separator('  Package                                                    Current   Target            URL '),
    {
      name: `@pnpm.e2e/multi-version-a                                    1.0.0 ❯ 1.0.${chalk.greenBright.bold('1')}                 `,
      value: '@pnpm.e2e/multi-version-a',
      short: '@pnpm.e2e/multi-version-a',
    },
    {
      name: `@pnpm.e2e/multi-version-c                                    3.0.0 ❯ 3.${chalk.yellowBright.bold('1.10')}                `,
      value: '@pnpm.e2e/multi-version-c',
      short: '@pnpm.e2e/multi-version-c',
    },
  ])
  expect(mockCheckbox).toHaveBeenCalledWith(
    expect.objectContaining({
      message:
        'Choose which dependencies to update ' +
        `(Press ${chalk.cyan('<space>')} to select, ` +
        `${chalk.cyan('<a>')} to toggle all, ` +
        `${chalk.cyan('<i>')} to invert selection)\n\nEnter to start updating. Ctrl-c to cancel.`,
      pageSize: process.stdout.rows == null ? 7 : Math.max(7, process.stdout.rows - 6),
    })
  )
  expect(callArgs.theme.style.highlight('focused row')).toBe('focused row')

  {
    const lockfile = project.readLockfile()

    expect(lockfile.packages['@pnpm.e2e/multi-version-c@3.0.0']).toBeTruthy()
    expect(lockfile.packages['@pnpm.e2e/multi-version-a@1.0.1']).toBeTruthy()
    expect(lockfile.packages['@pnpm.e2e/multi-version-b@2.0.0']).toBeTruthy()
  }

  // Update to latest versions
  mockCheckbox.mockClear()
  mockCheckbox.mockResolvedValue(['@pnpm.e2e/multi-version-a'])
  await update.handler({
    ...DEFAULT_OPTIONS,
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    interactive: true,
    latest: true,
    linkWorkspacePackages: true,
    storeDir,
  })

  // eslint-disable-next-line
  const callArgs2 = mockCheckbox.mock.calls[0][0] as any
  const flatChoices2 = callArgs2.choices

  expect(flatChoices2).toStrictEqual([
    new Separator(chalk.bold('── dependencies ──')),
    new Separator('  Package                                                    Current   Target            URL '),
    {
      name: `@pnpm.e2e/multi-version-a                                    1.0.1 ❯ ${chalk.redBright.bold('2.1.0')}                 `,
      value: '@pnpm.e2e/multi-version-a',
      short: '@pnpm.e2e/multi-version-a',
    },
    {
      name: `@pnpm.e2e/multi-version-b                                    2.0.0 ❯ ${chalk.redBright.bold('3.1.0')}                 `,
      value: '@pnpm.e2e/multi-version-b',
      short: '@pnpm.e2e/multi-version-b',
    },
    {
      name: `@pnpm.e2e/multi-version-c                                    3.0.0 ❯ ${chalk.redBright.bold('4.0.0')}                 `,
      value: '@pnpm.e2e/multi-version-c',
      short: '@pnpm.e2e/multi-version-c',
    },
  ])
  expect(mockCheckbox).toHaveBeenCalledWith(
    expect.objectContaining({
      message:
        'Choose which dependencies to update ' +
        `(Press ${chalk.cyan('<space>')} to select, ` +
        `${chalk.cyan('<a>')} to toggle all, ` +
        `${chalk.cyan('<i>')} to invert selection)\n\nEnter to start updating. Ctrl-c to cancel.`,
    })
  )

  {
    const lockfile = project.readLockfile()

    expect(lockfile.packages['@pnpm.e2e/multi-version-c@3.0.0']).toBeTruthy()
    expect(lockfile.packages['@pnpm.e2e/multi-version-a@2.1.0']).toBeTruthy()
    expect(lockfile.packages['@pnpm.e2e/multi-version-b@2.0.0']).toBeTruthy()
  }
})

test('interactive update of dev dependencies only', async () => {
  preparePackages([
    {
      name: 'project1',

      dependencies: {
        'is-negative': '^1.0.1',
      },
    },
    {
      name: 'project2',

      devDependencies: {
        'is-negative': '^1.0.0',
      },
    },
  ])
  const storeDir = path.resolve('store')

  mockCheckbox.mockResolvedValue(['is-negative'])

  const { allProjects, selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(
    process.cwd(),
    []
  )
  await install.handler({
    ...DEFAULT_OPTIONS,
    cacheDir: path.resolve('cache'),
    allProjects,
    dir: process.cwd(),
    linkWorkspacePackages: true,
    lockfileDir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    storeDir,
    workspaceDir: process.cwd(),
  })
  await update.handler({
    ...DEFAULT_OPTIONS,
    cacheDir: path.resolve('cache'),
    allProjects,
    cliOptions: {
      dev: true,
      optional: false,
      production: false,
    },
    dir: process.cwd(),
    interactive: true,
    latest: true,
    linkWorkspacePackages: true,
    lockfileDir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    storeDir,
    workspaceDir: process.cwd(),
  })

  const lockfile = readYamlFileSync<LockfileObject>('pnpm-lock.yaml')

  expect(Object.keys(lockfile.packages ?? {})).toStrictEqual([
    'is-negative@1.0.1',
    'is-negative@2.1.0',
  ])
})

test('interactively update should ignore dependencies from the ignoreDependencies field', async () => {
  const project = prepare({
    dependencies: {
      // has 1.0.0 and 1.0.1 that satisfy this range
      'is-negative': '^1.0.0',
      // only 2.0.0 satisfies this range
      'is-positive': '^2.0.0',
      // has many versions that satisfy ^3.0.0
      micromatch: '^3.0.0',
    },
  })

  const storeDir = path.resolve('pnpm-store')

  await add.handler(
    {
      ...DEFAULT_OPTIONS,
      cacheDir: path.resolve('cache'),
      dir: process.cwd(),
      linkWorkspacePackages: true,
      save: false,
      storeDir,
    },
    ['is-negative@1.0.0', 'is-positive@2.0.0', 'micromatch@3.0.0']
  )

  mockCheckbox.mockResolvedValue(['micromatch'])

  mockCheckbox.mockClear()
  await update.handler({
    ...DEFAULT_OPTIONS,
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    interactive: true,
    linkWorkspacePackages: true,
    storeDir,
    updateConfig: {
      ignoreDependencies: ['is-negative'],
    },
  })

  // eslint-disable-next-line
  const callArgs3 = mockCheckbox.mock.calls[0][0] as any
  const flatChoices3 = callArgs3.choices

  expect(flatChoices3).toStrictEqual(
    [
      new Separator(chalk.bold('── dependencies ──')),
      new Separator('  Package                                                    Current   Target            URL '),
      {
        name: `micromatch                                                   3.0.0 ❯ 3.${chalk.yellowBright.bold('1.10')}                `,
        value: 'micromatch',
        short: 'micromatch',
      },
    ]
  )

  expect(mockCheckbox).toHaveBeenCalledWith(
    expect.objectContaining({
      message:
        'Choose which dependencies to update ' +
        `(Press ${chalk.cyan('<space>')} to select, ` +
        `${chalk.cyan('<a>')} to toggle all, ` +
        `${chalk.cyan('<i>')} to invert selection)\n\nEnter to start updating. Ctrl-c to cancel.`,
    })
  )

  {
    const lockfile = project.readLockfile()

    expect(lockfile.packages['micromatch@3.1.10']).toBeTruthy()
    expect(lockfile.packages['is-negative@1.0.0']).toBeTruthy()
    expect(lockfile.packages['is-positive@2.0.0']).toBeTruthy()
  }
})
