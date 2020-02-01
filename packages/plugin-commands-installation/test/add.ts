import PnpmError from '@pnpm/error'
import { add } from '@pnpm/plugin-commands-installation'
import { preparePackages } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import path = require('path')
import test = require('tape')

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

test('installing with "workspace:" should work even if link-workspace-packages is off', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '2.0.0',
    },
  ])

  await add.handler(['project-2@workspace:*'], {
    ...DEFAULT_OPTIONS,
    dir: path.resolve('project-1'),
    linkWorkspacePackages: false,
    saveWorkspaceProtocol: false,
    workspaceDir: process.cwd(),
  })

  const pkg = await import(path.resolve('project-1/package.json'))

  t.deepEqual(pkg && pkg.dependencies, { 'project-2': 'workspace:^2.0.0' })

  await projects['project-1'].has('project-2')

  t.end()
})

test('installing with "workspace=true" should work even if link-workspace-packages is off and save-workspace-protocol is false', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '2.0.0',
    },
  ])

  await add.handler(['project-2'], {
    ...DEFAULT_OPTIONS,
    dir: path.resolve('project-1'),
    linkWorkspacePackages: false,
    saveWorkspaceProtocol: false,
    workspace: true,
    workspaceDir: process.cwd(),
  })

  const pkg = await import(path.resolve('project-1/package.json'))

  t.deepEqual(pkg && pkg.dependencies, { 'project-2': 'workspace:^2.0.0' })

  await projects['project-1'].has('project-2')

  t.end()
})

test('add: fail when "workspace" option is true but the command runs not in a workspace', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '2.0.0',
    },
  ])

  let err!: PnpmError
  try {
    await add.handler(['project-2'], {
      ...DEFAULT_OPTIONS,
      dir: path.resolve('project-1'),
      linkWorkspacePackages: false,
      saveWorkspaceProtocol: false,
      workspace: true,
    })
  } catch (_err) {
    err = _err
  }
  t.equal(err.code, 'ERR_PNPM_WORKSPACE_OPTION_OUTSIDE_WORKSPACE')
  t.equal(err.message, '--workspace can only be used inside a workspace')
  t.end()
})

test('add: fail when "workspace" option is true but linkWorkspacePackages is false and --no-save-workspace-protocol option is used', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '2.0.0',
    },
  ])

  let err!: PnpmError
  try {
    await add.handler(['project-2'], {
      ...DEFAULT_OPTIONS,
      dir: path.resolve('project-1'),
      linkWorkspacePackages: false,
      rawLocalConfig: {
        ...DEFAULT_OPTIONS.rawLocalConfig,
        'save-workspace-protocol': false,
      },
      saveWorkspaceProtocol: false,
      workspace: true,
      workspaceDir: process.cwd(),
    })
  } catch (_err) {
    err = _err
  }
  t.equal(err.code, 'ERR_PNPM_BAD_OPTIONS')
  t.ok(err.message.startsWith('This workspace has link-workspace-packages turned off'))
  t.end()
})

test('installing with "workspace=true" with linkWorkpacePackages on and saveWorkspaceProtocol off', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '2.0.0',
    },
  ])

  await add.handler(['project-2'], {
    ...DEFAULT_OPTIONS,
    dir: path.resolve('project-1'),
    linkWorkspacePackages: true,
    saveWorkspaceProtocol: false,
    workspace: true,
    workspaceDir: process.cwd(),
  })

  const pkg = await import(path.resolve('project-1/package.json'))

  t.deepEqual(pkg && pkg.dependencies, { 'project-2': '^2.0.0' })

  await projects['project-1'].has('project-2')

  t.end()
})

test('add: fail when --no-save option is used', async (t) => {
  let err!: PnpmError
  try {
    await add.handler(['is-positive'], {
      ...DEFAULT_OPTIONS,
      cliOptions: {
        save: false,
      },
      dir: process.cwd(),
      linkWorkspacePackages: false,
    })
  } catch (_err) {
    err = _err
  }
  t.equal(err.code, 'ERR_PNPM_OPTION_NOT_SUPPORTED')
  t.equal(err.message, 'The "add" command currently does not support the no-save option')
  t.end()
})
