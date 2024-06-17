import path from 'path'
import { filterPackagesFromDir } from '@pnpm/workspace.filter-packages-from-dir'
import { type Lockfile } from '@pnpm/lockfile-types'
import { add, install, update } from '@pnpm/plugin-commands-installation'
import { prepare, preparePackages } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { sync as readYamlFile } from 'read-yaml-file'
import chalk from 'chalk'
import * as enquirer from 'enquirer'

jest.mock('enquirer', () => ({ prompt: jest.fn() }))

// eslint-disable-next-line
const prompt = enquirer.prompt as any

const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}`

const DEFAULT_OPTIONS = {
  argv: {
    original: [],
  },
  bail: false,
  bin: 'node_modules/.bin',
  extraEnv: {},
  cliOptions: {},
  deployAllFiles: false,
  include: {
    dependencies: true,
    devDependencies: true,
    optionalDependencies: true,
  },
  lock: true,
  pnpmfile: '.pnpmfile.cjs',
  pnpmHomeDir: '',
  rawConfig: { registry: REGISTRY_URL },
  rawLocalConfig: { registry: REGISTRY_URL },
  registries: {
    default: REGISTRY_URL,
  },
  rootProjectManifestDir: '',
  sort: true,
  userConfig: {},
  workspaceConcurrency: 1,
  virtualStoreDirMaxLength: 120,
}

test('interactively update', async () => {
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

  const headerChoice = {
    name: 'Package                                                    Current   Target            URL ',
    disabled: true,
    hint: '',
    value: '',
  }

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

  prompt.mockResolvedValue({
    updateDependencies: [
      {
        value: 'is-negative',
        name: chalk`is-negative 1.0.0 ❯ 1.0.{greenBright.bold 1} https://pnpm.io/ `,
      },
    ],
  })

  prompt.mockClear()
  // t.comment('update to compatible versions')
  await update.handler({
    ...DEFAULT_OPTIONS,
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    interactive: true,
    linkWorkspacePackages: true,
    storeDir,
  })

  expect(prompt.mock.calls[0][0].choices).toStrictEqual([
    {
      choices: [
        headerChoice,
        {
          message: chalk`is-negative                                                  1.0.0 ❯ 1.0.{greenBright.bold 1}                 `,
          value: 'is-negative',
          name: 'is-negative',
        },
        {
          message: chalk`micromatch                                                   3.0.0 ❯ 3.{yellowBright.bold 1.10}                `,
          value: 'micromatch',
          name: 'micromatch',
        },
      ],
      name: '[dependencies]',
      message: 'dependencies',
    },
  ])
  expect(prompt).toHaveBeenCalledWith(
    expect.objectContaining({
      footer: '\nEnter to start updating. Ctrl-c to cancel.',
      message:
        'Choose which packages to update ' +
        `(Press ${chalk.cyan('<space>')} to select, ` +
        `${chalk.cyan('<a>')} to toggle all, ` +
        `${chalk.cyan('<i>')} to invert selection)`,
      name: 'updateDependencies',
      type: 'multiselect',
    })
  )

  {
    const lockfile = project.readLockfile()

    expect(lockfile.packages['micromatch@3.0.0']).toBeTruthy()
    expect(lockfile.packages['is-negative@1.0.1']).toBeTruthy()
    expect(lockfile.packages['is-positive@2.0.0']).toBeTruthy()
  }

  // t.comment('update to latest versions')
  prompt.mockClear()
  await update.handler({
    ...DEFAULT_OPTIONS,
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    interactive: true,
    latest: true,
    linkWorkspacePackages: true,
    storeDir,
  })

  expect(prompt.mock.calls[0][0].choices).toStrictEqual([
    {
      choices: [
        headerChoice,
        {
          message: chalk`is-negative                                                  1.0.1 ❯ {redBright.bold 2.1.0}                 `,
          value: 'is-negative',
          name: 'is-negative',
        },
        {
          message: chalk`is-positive                                                  2.0.0 ❯ {redBright.bold 3.1.0}                 `,
          value: 'is-positive',
          name: 'is-positive',
        },
        {
          message: chalk`micromatch                                                   3.0.0 ❯ {redBright.bold 4.0.7}                 `,
          value: 'micromatch',
          name: 'micromatch',
        },
      ],
      name: '[dependencies]',
      message: 'dependencies',
    },
  ])
  expect(prompt).toHaveBeenCalledWith(
    expect.objectContaining({
      footer: '\nEnter to start updating. Ctrl-c to cancel.',
      message:
        'Choose which packages to update ' +
        `(Press ${chalk.cyan('<space>')} to select, ` +
        `${chalk.cyan('<a>')} to toggle all, ` +
        `${chalk.cyan('<i>')} to invert selection)`,
      name: 'updateDependencies',
      type: 'multiselect',
    })
  )

  {
    const lockfile = project.readLockfile()

    expect(lockfile.packages['micromatch@3.0.0']).toBeTruthy()
    expect(lockfile.packages['is-negative@2.1.0']).toBeTruthy()
    expect(lockfile.packages['is-positive@2.0.0']).toBeTruthy()
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

  prompt.mockResolvedValue({
    updateDependencies: [
      {
        value: 'is-negative',
        name: chalk`is-negative 1.0.0 ❯ 1.0.{greenBright.bold 1} https://pnpm.io/ `,
      },
    ],
  })

  const { allProjects, selectedProjectsGraph } = await filterPackagesFromDir(
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

  const lockfile = readYamlFile<Lockfile>('pnpm-lock.yaml')

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
    pnpm: {
      updateConfig: {
        ignoreDependencies: ['is-negative'],
      },
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

  prompt.mockResolvedValue({
    updateDependencies: [{ value: 'micromatch', name: 'anything' }],
  })

  prompt.mockClear()
  await update.handler({
    ...DEFAULT_OPTIONS,
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    interactive: true,
    linkWorkspacePackages: true,
    storeDir,
  })

  expect(prompt.mock.calls[0][0].choices).toStrictEqual(
    [
      {
        choices: [
          {
            disabled: true,
            hint: '',
            name: 'Package                                                    Current   Target            URL ',
            value: '',
          },
          {
            message: chalk`micromatch                                                   3.0.0 ❯ 3.{yellowBright.bold 1.10}                `,
            value: 'micromatch',
            name: 'micromatch',
          },
        ],
        name: '[dependencies]',
        message: 'dependencies',
      },
    ]
  )

  expect(prompt).toHaveBeenCalledWith(
    expect.objectContaining({
      footer: '\nEnter to start updating. Ctrl-c to cancel.',
      message:
        'Choose which packages to update ' +
        `(Press ${chalk.cyan('<space>')} to select, ` +
        `${chalk.cyan('<a>')} to toggle all, ` +
        `${chalk.cyan('<i>')} to invert selection)`,
      name: 'updateDependencies',
      type: 'multiselect',
    })
  )

  {
    const lockfile = project.readLockfile()

    expect(lockfile.packages['micromatch@3.1.10']).toBeTruthy()
    expect(lockfile.packages['is-negative@1.0.0']).toBeTruthy()
    expect(lockfile.packages['is-positive@2.0.0']).toBeTruthy()
  }
})
