import fs from 'fs'
import path from 'path'
import { type PnpmError } from '@pnpm/error'
import { add, remove } from '@pnpm/plugin-commands-installation'
import { prepare, prepareEmpty, preparePackages } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { loadJsonFile } from 'load-json-file'
import { temporaryDirectory } from 'tempy'
import { sync as writeYamlFile } from 'write-yaml-file'

const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}`
const tmp = temporaryDirectory()

const DEFAULT_OPTIONS = {
  argv: {
    original: [],
  },
  bail: false,
  bin: 'node_modules/.bin',
  cacheDir: path.join(tmp, 'cache'),
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
  preferWorkspacePackages: true,
  pnpmfile: ['.pnpmfile.cjs'],
  pnpmHomeDir: '',
  rawConfig: { registry: REGISTRY_URL },
  rawLocalConfig: { registry: REGISTRY_URL },
  registries: {
    default: REGISTRY_URL,
  },
  rootProjectManifestDir: '',
  sort: true,
  storeDir: path.join(tmp, 'store'),
  userConfig: {},
  workspaceConcurrency: 1,
  virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
}

const describeOnLinuxOnly = process.platform === 'linux' ? describe : describe.skip

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

  const { default: pkg } = await import(path.resolve('project-1/package.json'))

  expect(pkg?.dependencies).toEqual({ 'project-2': 'workspace:^2.0.0' })

  projects['project-1'].has('project-2')
})

test('installing with "workspace:" should work even if link-workspace-packages is off and save-workspace-protocol is "rolling"', async () => {
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
    saveWorkspaceProtocol: 'rolling',
    workspaceDir: process.cwd(),
  }, ['project-2@workspace:*'])

  const { default: pkg } = await import(path.resolve('project-1/package.json'))

  expect(pkg?.dependencies).toEqual({ 'project-2': 'workspace:*' })

  projects['project-1'].has('project-2')
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

  const { default: pkg } = await import(path.resolve('project-1/package.json'))

  expect(pkg?.dependencies).toEqual({ 'project-2': 'workspace:^2.0.0' })

  projects['project-1'].has('project-2')
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

  const { default: pkg } = await import(path.resolve('project-1/package.json'))

  expect(pkg?.dependencies).toEqual({ 'project-2': 'workspace:^2.0.0' })

  projects['project-1'].has('project-2')
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

  project.has('is-positive')

  await remove.handler({
    ...DEFAULT_OPTIONS,
    dir: process.cwd(),
    linkWorkspacePackages: false,
  }, ['is-positive'])

  project.hasNot('is-positive')

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
    const { default: manifest } = (await import(path.resolve('package.json')))

    expect(
      manifest.dependencies['is-positive']
    ).toMatch(/~(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Z-]+(?:\.[0-9A-Z-]+)*))?(?:\+[0-9A-Z-]+)?$/i)
  }
})

test('pnpm add automatically installs missing peer dependencies', async () => {
  const project = prepare()
  await add.handler({
    ...DEFAULT_OPTIONS,
    autoInstallPeers: true,
    dir: process.cwd(),
    linkWorkspacePackages: false,
  }, ['@pnpm.e2e/abc@1.0.0'])

  const lockfile = project.readLockfile()
  expect(Object.keys(lockfile.packages)).toHaveLength(5)
})

test('add: fail when global bin directory is not found', async () => {
  prepareEmpty()

  let err!: PnpmError
  try {
    await add.handler({
      ...DEFAULT_OPTIONS,
      bin: undefined as any, // eslint-disable-line
      dir: path.resolve('project-1'),
      global: true,
      linkWorkspacePackages: false,
      saveWorkspaceProtocol: false,
      workspace: true,
    }, ['@pnpm.e2e/hello-world-js-bin'])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err.code).toBe('ERR_PNPM_NO_GLOBAL_BIN_DIR')
})

test('add: fail trying to install pnpm', async () => {
  prepareEmpty()

  let err!: PnpmError
  try {
    await add.handler({
      ...DEFAULT_OPTIONS,
      bin: path.resolve('project/bin'),
      dir: path.resolve('project'),
      global: true,
      linkWorkspacePackages: false,
      saveWorkspaceProtocol: false,
      workspace: false,
    }, ['pnpm'])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err.code).toBe('ERR_PNPM_GLOBAL_PNPM_INSTALL')
})

test('add: fail trying to install @pnpm/exe', async () => {
  prepareEmpty()

  let err!: PnpmError
  try {
    await add.handler({
      ...DEFAULT_OPTIONS,
      bin: path.resolve('project/bin'),
      dir: path.resolve('project'),
      global: true,
      linkWorkspacePackages: false,
      saveWorkspaceProtocol: false,
      workspace: false,
    }, ['@pnpm/exe'])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err.code).toBe('ERR_PNPM_GLOBAL_PNPM_INSTALL')
})

test('minimumReleaseAge makes install fail if there is no version that was published before the cutoff', async () => {
  prepareEmpty()

  const isOdd011ReleaseDate = new Date(2016, 11, 7 - 2) // 0.1.1 was released at 2016-12-07T07:18:01.205Z
  const diff = Date.now() - isOdd011ReleaseDate.getTime()
  const minimumReleaseAge = diff / (60 * 1000) // converting to minutes

  await expect(add.handler({
    ...DEFAULT_OPTIONS,
    dir: path.resolve('project'),
    minimumReleaseAge,
    linkWorkspacePackages: false,
  }, ['is-odd@0.1.1'])).rejects.toThrow(/Version 0\.1\.1 \(released .+\) of is-odd does not meet the minimumReleaseAge constraint/)
})

describeOnLinuxOnly('filters optional dependencies based on supportedArchitectures.libc', () => {
  test.each([
    ['glibc', '@pnpm.e2e+only-linux-x64-glibc@1.0.0', '@pnpm.e2e+only-linux-x64-musl@1.0.0'],
    ['musl', '@pnpm.e2e+only-linux-x64-musl@1.0.0', '@pnpm.e2e+only-linux-x64-glibc@1.0.0'],
  ])('%p → installs %p, does not install %p', async (libc, found, notFound) => {
    const supportedArchitectures = {
      os: ['linux'],
      cpu: ['x64'],
      libc: [libc],
    }

    prepare()

    writeYamlFile('pnpm-workspace.yaml', {
      supportedArchitectures,
    })

    await add.handler({
      ...DEFAULT_OPTIONS,
      supportedArchitectures,
      dir: process.cwd(),
      linkWorkspacePackages: true,
    }, ['@pnpm.e2e/support-different-architectures'])

    const pkgDirs = fs.readdirSync(path.resolve('node_modules', '.pnpm'))
    expect(pkgDirs).toContain('@pnpm.e2e+support-different-architectures@1.0.0')
    expect(pkgDirs).toContain(found)
    expect(pkgDirs).not.toContain(notFound)
  })
})
