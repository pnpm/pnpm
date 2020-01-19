import { add } from '@pnpm/plugin-commands-installation'
import prepare from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import chalk = require('chalk')
import path = require('path')
import proxyquire = require('proxyquire')
import R = require('ramda')
import sinon = require('sinon')
import test = require('tape')

const prompt = sinon.stub()

const update = proxyquire('../../lib/update', {
  'enquirer': { prompt },
})

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

test('interactively update', async (t) => {
  const project = prepare(t, {
    dependencies: {
      // has 1.0.0 and 1.0.1 that satisfy this range
      'is-negative': '^1.0.0',
      // only 2.0.0 satisfies this range
      'is-positive': '^2.0.0',
      // has many versions that satisfy ^3.0.0
      'micromatch': '^3.0.0',
    },
  })

  await add.handler([
    'is-negative@1.0.0',
    'is-positive@2.0.0',
    'micromatch@3.0.0',
  ], {
    ...DEFAULT_OPTIONS,
    dir: process.cwd(),
    linkWorkspacePackages: true,
    save: false,
  })

  prompt.returns({
    updateDependencies: ['is-negative'],
  })

  t.comment('update to compatible versions')
  await update.handler([], {
    ...DEFAULT_OPTIONS,
    dir: process.cwd(),
    interactive: true,
    linkWorkspacePackages: true,
  })

  t.ok(prompt.calledWithMatch({
    choices: [
      {
        message: chalk`is-negative 1.0.0 ❯ 1.0.{greenBright.bold 1}  `,
        name: 'is-negative',
      },
      {
        message: chalk`micromatch  3.0.0 ❯ 3.{yellowBright.bold 1.10} `,
        name: 'micromatch',
      },
    ],
    footer: '\nEnter to start updating. Ctrl-c to cancel.',
    message: `Choose which packages to update ` +
        `(Press ${chalk.cyan('<space>')} to select, ` +
        `${chalk.cyan('<a>')} to toggle all, ` +
        `${chalk.cyan('<i>')} to invert selection)`,
    name: 'updateDependencies',
    type: 'multiselect',
  }))

  {
    const lockfile = await project.readLockfile()

    t.ok(lockfile.packages['/micromatch/3.0.0'])
    t.ok(lockfile.packages['/is-negative/1.0.1'])
    t.ok(lockfile.packages['/is-positive/2.0.0'])
  }

  t.comment('update to latest versions')
  await update.handler([], {
    ...DEFAULT_OPTIONS,
    dir: process.cwd(),
    interactive: true,
    latest: true,
    linkWorkspacePackages: true,
  })

  t.ok(prompt.calledWithMatch({
    choices: [
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
    ],
    footer: '\nEnter to start updating. Ctrl-c to cancel.',
    message: `Choose which packages to update ` +
        `(Press ${chalk.cyan('<space>')} to select, ` +
        `${chalk.cyan('<a>')} to toggle all, ` +
        `${chalk.cyan('<i>')} to invert selection)`,
    name: 'updateDependencies',
    type: 'multiselect',
  }))

  {
    const lockfile = await project.readLockfile()

    t.ok(lockfile.packages['/micromatch/3.0.0'])
    t.ok(lockfile.packages['/is-negative/2.1.0'])
    t.ok(lockfile.packages['/is-positive/2.0.0'])
  }

  t.end()
})
