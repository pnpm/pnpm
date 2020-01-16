import { add } from '@pnpm/plugin-commands-installation'
import prepare from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import path = require('path')
import proxyquire = require('proxyquire')
import sinon = require('sinon')
import test = require('tape')

const prompt = sinon.stub()

const update = proxyquire('../lib/update', {
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

  t.equal(prompt.args[0][0].choices.length, 1)
  t.equal(prompt.args[0][0].choices[0].choices.length, 2)

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

  t.equal(prompt.args[1][0].choices.length, 1)
  t.equal(prompt.args[1][0].choices[0].choices.length, 3)

  {
    const lockfile = await project.readLockfile()

    t.ok(lockfile.packages['/micromatch/3.0.0'])
    t.ok(lockfile.packages['/is-negative/2.1.0'])
    t.ok(lockfile.packages['/is-positive/2.0.0'])
  }

  t.end()
})
