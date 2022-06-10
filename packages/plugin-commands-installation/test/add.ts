import path from 'path'
import PnpmError from '@pnpm/error'
import { add, remove } from '@pnpm/plugin-commands-installation'
import prepare, { preparePackages } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import loadJsonFile from 'load-json-file'
import tempy from 'tempy'

const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}`
const tmp = tempy.directory()

const DEFAULT_OPTIONS = {
  argv: {
    original: [],
  },
  bail: false,
  bin: 'node_modules/.bin',
  cacheDir: path.join(tmp, 'cache'),
  cliOptions: {},
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
  sort: true,
  storeDir: path.join(tmp, 'store'),
  userConfig: {},
  workspaceConcurrency: 1,
}

test('installing with "workspace:" should work even if link-workspace-packages is off', async () => {
  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '2.0.0',
    },
  ])

  await add.handler({
    ...DEFAULT_OPTIONS,
    dir: path.resolve('project-1'),
    linkWorkspacePackages: false,
    saveWorkspaceProtocol: false,
    workspaceDir: process.cwd(),
  }, ['project-2@workspace:*'])

  const pkg = await import(path.resolve('project-1/package.json'))

  expect(pkg?.dependencies).toStrictEqual({ 'project-2': 'workspace:^2.0.0' })

  await projects['project-1'].has('project-2')
})

test('installing with "workspace=true" should work even if link-workspace-packages is off and save-workspace-protocol is false', async () => {
  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '2.0.0',
    },
  ])

  await add.handler({
    ...DEFAULT_OPTIONS,
    dir: path.resolve('project-1'),
    linkWorkspacePackages: false,
    saveWorkspaceProtocol: false,
    workspace: true,
    workspaceDir: process.cwd(),
  }, ['project-2'])

  const pkg = await import(path.resolve('project-1/package.json'))

  expect(pkg?.dependencies).toStrictEqual({ 'project-2': 'workspace:^2.0.0' })

  await projects['project-1'].has('project-2')
})

test('add: fail when "workspace" option is true but the command runs not in a workspace', async () => {
  preparePackages([
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
    await add.handler({
      ...DEFAULT_OPTIONS,
      dir: path.resolve('project-1'),
      linkWorkspacePackages: false,
      saveWorkspaceProtocol: false,
      workspace: true,
    }, ['project-2'])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err.code).toBe('ERR_PNPM_WORKSPACE_OPTION_OUTSIDE_WORKSPACE')
  expect(err.message).toBe('--workspace can only be used inside a workspace')
})

test('add: fail when "workspace" option is true but linkWorkspacePackages is false and --no-save-workspace-protocol option is used', async () => {
  preparePackages([
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
    await add.handler({
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
    }, ['project-2'])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err.code).toBe('ERR_PNPM_BAD_OPTIONS')
  expect(err.message.startsWith('This workspace has link-workspace-packages turned off')).toBeTruthy()
})

test('installing with "workspace=true" with linkWorkspacePackages on and saveWorkspaceProtocol off', async () => {
  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '2.0.0',
    },
  ])

  await add.handler({
    ...DEFAULT_OPTIONS,
    dir: path.resolve('project-1'),
    linkWorkspacePackages: true,
    saveWorkspaceProtocol: false,
    workspace: true,
    workspaceDir: process.cwd(),
  }, ['project-2'])

  const pkg = await import(path.resolve('project-1/package.json'))

  expect(pkg?.dependencies).toStrictEqual({ 'project-2': '^2.0.0' })

  await projects['project-1'].has('project-2')
})

test('add: fail when --no-save option is used', async () => {
  let err!: PnpmError
  try {
    await add.handler({
      ...DEFAULT_OPTIONS,
      cliOptions: {
        save: false,
      },
      dir: process.cwd(),
      linkWorkspacePackages: false,
    }, ['is-positive'])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err.code).toBe('ERR_PNPM_OPTION_NOT_SUPPORTED')
  expect(err.message).toBe('The "add" command currently does not support the no-save option')
})

test('pnpm add --save-peer', async () => {
  const project = prepare()

  await add.handler({
    ...DEFAULT_OPTIONS,
    dir: process.cwd(),
    linkWorkspacePackages: false,
    savePeer: true,
  }, ['is-positive@1.0.0'])

  {
    const manifest = await loadJsonFile(path.resolve('package.json'))

    expect(
      manifest
    ).toStrictEqual(
      {
        name: 'project',
        version: '0.0.0',

        devDependencies: { 'is-positive': '1.0.0' },
        peerDependencies: { 'is-positive': '1.0.0' },
      }
    )
  }

  await project.has('is-positive')

  await remove.handler({
    ...DEFAULT_OPTIONS,
    dir: process.cwd(),
    linkWorkspacePackages: false,
  }, ['is-positive'])

  await project.hasNot('is-positive')

  {
    const manifest = await loadJsonFile(path.resolve('package.json'))

    expect(
      manifest
    ).toStrictEqual(
      {
        name: 'project',
        version: '0.0.0',
      }
    )
  }
})

test('pnpm add - with save-prefix set to empty string should save package version without prefix', async () => {
  prepare()
  await add.handler({
    ...DEFAULT_OPTIONS,
    dir: process.cwd(),
    linkWorkspacePackages: false,
    savePrefix: '',
  }, ['is-positive@1.0.0'])

  {
    const manifest = await loadJsonFile(path.resolve('package.json'))

    expect(
      manifest
    ).toStrictEqual(
      {
        name: 'project',
        version: '0.0.0',
        dependencies: { 'is-positive': '1.0.0' },
      }
    )
  }
})

test('pnpm add - should add prefix when set in .npmrc when a range is not specified explicitly', async () => {
  prepare()
  await add.handler({
    ...DEFAULT_OPTIONS,
    dir: process.cwd(),
    linkWorkspacePackages: false,
    savePrefix: '~',
  }, ['is-positive'])

  {
    const manifest = (await import(path.resolve('package.json')))

    expect(
      manifest.dependencies['is-positive']
    ).toMatch(/~([0-9]+)\.([0-9]+)\.([0-9]+)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+[0-9A-Za-z-]+)?$/)
  }
})

test('pnpm add automatically installs missing peer dependencies', async () => {
  const project = prepare()
  await add.handler({
    ...DEFAULT_OPTIONS,
    autoInstallPeers: true,
    dir: process.cwd(),
    linkWorkspacePackages: false,
  }, ['abc@1.0.0'])

  const lockfile = await project.readLockfile()
  expect(Object.keys(lockfile.packages).length).toBe(5)
})
