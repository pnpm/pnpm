import { readProjects } from '@pnpm/filter-workspace-packages'
import { Lockfile } from '@pnpm/lockfile-types'
import prepare, { preparePackages } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import readYamlFile from 'read-yaml-file'
import path = require('path')
import chalk = require('chalk')

jest.setTimeout(10000)

jest.mock('enquirer', () => ({ prompt: jest.fn() }))

// eslint-disable-next-line
import * as enquirer from 'enquirer'

// eslint-disable-next-line
const prompt = enquirer.prompt as any

// eslint-disable-next-line
import { add, install, update } from '@pnpm/plugin-commands-installation'

const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}`

const DEFAULT_OPTIONS = {
  argv: {
    original: [],
  },
  bail: false,
  cliOptions: {},
  include: {
    dependencies: true,
    devDependencies: true,
    optionalDependencies: true,
  },
  lock: true,
  pnpmfile: 'pnpmfile.js',
  rawConfig: { registry: REGISTRY_URL },
  rawLocalConfig: { registry: REGISTRY_URL },
  registries: {
    default: REGISTRY_URL,
  },
  sort: true,
  workspaceConcurrency: 1,
}

test('interactively update', async () => {
  const project = prepare(undefined, {
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
  await add.handler({
    ...DEFAULT_OPTIONS,
    dir: process.cwd(),
    linkWorkspacePackages: true,
    save: false,
    storeDir,
  }, [
    'is-negative@1.0.0',
    'is-positive@2.0.0',
    'micromatch@3.0.0',
  ])

  prompt.mockResolvedValue({
    updateDependencies: ['is-negative'],
  })

  // t.comment('update to compatible versions')
  await update.handler({
    ...DEFAULT_OPTIONS,
    dir: process.cwd(),
    interactive: true,
    linkWorkspacePackages: true,
    storeDir,
  })

  expect(prompt.mock.calls[0][0].choices).toStrictEqual([
    {
      message: chalk`is-negative 1.0.0 ❯ 1.0.{greenBright.bold 1}  `,
      name: 'is-negative',
    },
    {
      message: chalk`micromatch  3.0.0 ❯ 3.{yellowBright.bold 1.10} `,
      name: 'micromatch',
    },
  ])
  expect(prompt).toBeCalledWith(expect.objectContaining({
    footer: '\nEnter to start updating. Ctrl-c to cancel.',
    message: 'Choose which packages to update ' +
        `(Press ${chalk.cyan('<space>')} to select, ` +
        `${chalk.cyan('<a>')} to toggle all, ` +
        `${chalk.cyan('<i>')} to invert selection)`,
    name: 'updateDependencies',
    type: 'multiselect',
  }))

  {
    const lockfile = await project.readLockfile()

    expect(lockfile.packages['/micromatch/3.0.0']).toBeTruthy()
    expect(lockfile.packages['/is-negative/1.0.1']).toBeTruthy()
    expect(lockfile.packages['/is-positive/2.0.0']).toBeTruthy()
  }

  // t.comment('update to latest versions')
  prompt.mockClear()
  await update.handler({
    ...DEFAULT_OPTIONS,
    dir: process.cwd(),
    interactive: true,
    latest: true,
    linkWorkspacePackages: true,
    storeDir,
  })

  expect(prompt.mock.calls[0][0].choices).toStrictEqual([
    {
      message: chalk`is-negative 1.0.1 ❯ {redBright.bold 2.1.0} `,
      name: 'is-negative',
    },
    {
      message: chalk`is-positive 2.0.0 ❯ {redBright.bold 3.1.0} `,
      name: 'is-positive',
    },
    {
      message: chalk`micromatch  3.0.0 ❯ {redBright.bold 4.0.2} `,
      name: 'micromatch',
    },
  ])
  expect(prompt).toBeCalledWith(expect.objectContaining({
    footer: '\nEnter to start updating. Ctrl-c to cancel.',
    message: 'Choose which packages to update ' +
        `(Press ${chalk.cyan('<space>')} to select, ` +
        `${chalk.cyan('<a>')} to toggle all, ` +
        `${chalk.cyan('<i>')} to invert selection)`,
    name: 'updateDependencies',
    type: 'multiselect',
  }))

  {
    const lockfile = await project.readLockfile()

    expect(lockfile.packages['/micromatch/3.0.0']).toBeTruthy()
    expect(lockfile.packages['/is-negative/2.1.0']).toBeTruthy()
    expect(lockfile.packages['/is-positive/2.0.0']).toBeTruthy()
  }
})

test('interactive update of dev dependencies only', async () => {
  preparePackages(undefined, [
    {
      name: 'project1',

      dependencies: {
        'is-negative': '^1.0.0',
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
    updateDependencies: ['is-negative'],
  })

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTIONS,
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
    allProjects,
    dev: true,
    dir: process.cwd(),
    interactive: true,
    latest: true,
    linkWorkspacePackages: true,
    lockfileDir: process.cwd(),
    optional: false,
    production: false,
    recursive: true,
    selectedProjectsGraph,
    storeDir,
    workspaceDir: process.cwd(),
  })

  const lockfile = await readYamlFile<Lockfile>('pnpm-lock.yaml')

  expect(
    Object.keys(lockfile.packages ?? {})
  ).toStrictEqual(
    ['/is-negative/1.0.1', '/is-negative/2.1.0']
  )
})
