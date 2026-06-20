import { beforeEach, expect, jest, test } from '@jest/globals'
import type { Config, ConfigContext } from '@pnpm/config.reader'
import type { EnvLockfile } from '@pnpm/lockfile.types'

const closeStore = jest.fn<() => Promise<void>>(async () => {})
const createStoreController = jest.fn<(opts: object) => Promise<{
  ctrl: { close: typeof closeStore }
  dir: string
}>>(async () => ({
  ctrl: { close: closeStore },
  dir: '/store',
}))
const envLockfile: EnvLockfile = {
  importers: {
    '.': {
      configDependencies: {},
      packageManagerDependencies: {
        '@pnpm/exe': { specifier: '9.3.0', version: '9.3.0' },
        pnpm: { specifier: '9.3.0', version: '9.3.0' },
      },
    },
  },
  lockfileVersion: '9.0',
  packages: {
    '@pnpm/exe@9.3.0': {
      resolution: {
        integrity: 'sha512-exe',
      },
    },
    '@pnpm/linux-x64@9.3.0': {
      resolution: {
        integrity: 'sha512-linux-x64',
      },
    },
    'pnpm@9.3.0': {
      resolution: {
        integrity: 'sha512-pnpm',
      },
    },
  },
  snapshots: {
    '@pnpm/exe@9.3.0': {
      optionalDependencies: {
        '@pnpm/linux-x64': '9.3.0',
      },
    },
    '@pnpm/linux-x64@9.3.0': {
      optional: true,
    },
    'pnpm@9.3.0': {},
  },
}
const installPnpmToStore = jest.fn<(version: string, opts: object) => Promise<{ binDir: string }>>(async () => ({ binDir: '/store/bin' }))
const readEnvLockfile = jest.fn<(rootDir: string) => Promise<EnvLockfile | null>>(async () => envLockfile)
const resolvePackageManagerIntegrities = jest.fn<(version: string, opts: object) => Promise<EnvLockfile>>(async () => envLockfile)
const spawnSync = jest.fn(() => ({ status: 0 }))

jest.unstable_mockModule('@pnpm/cli.meta', () => ({
  packageManager: { name: 'pnpm', version: '11.0.0' },
}))
jest.unstable_mockModule('@pnpm/engine.pm.commands', () => ({
  installPnpmToStore,
}))
jest.unstable_mockModule('@pnpm/installing.env-installer', () => ({
  isPackageManagerResolved: () => true,
  resolvePackageManagerIntegrities,
}))
jest.unstable_mockModule('@pnpm/lockfile.fs', () => ({
  readEnvLockfile,
}))
jest.unstable_mockModule('@pnpm/shell.path', () => ({
  prependDirsToPath: () => ({ name: 'PATH', updated: true, value: '/store/bin' }),
}))
jest.unstable_mockModule('@pnpm/store.connection-manager', () => ({
  createStoreController,
}))
jest.unstable_mockModule('cross-spawn', () => ({
  default: { sync: spawnSync },
}))

const { switchCliVersion } = await import('./switchCliVersion.js')

beforeEach(() => {
  closeStore.mockClear()
  createStoreController.mockClear()
  installPnpmToStore.mockClear()
  readEnvLockfile.mockClear()
  readEnvLockfile.mockResolvedValue(envLockfile)
  resolvePackageManagerIntegrities.mockClear()
  resolvePackageManagerIntegrities.mockResolvedValue(envLockfile)
  spawnSync.mockClear()
})

test('switchCliVersion uses trusted package-manager registries instead of project registries', async () => {
  const exit = jest.spyOn(process, 'exit').mockImplementation(((code?: string | number | null | undefined) => {
    throw new Error(`exit ${code ?? 0}`)
  }) as typeof process.exit)

  const projectRegistries = {
    '@pnpm': 'https://project-pnpm.example.com/',
    default: 'https://project.example.com/',
  }
  const packageManagerRegistries = {
    '@pnpm': 'https://trusted-pnpm.example.com/',
    default: 'https://trusted.example.com/',
  }
  const packageManagerNetworkConfig = {
    configByUri: {
      '//trusted.example.com/': { '@': { authToken: 'trusted-token' } },
    },
    httpProxy: 'http://trusted-http-proxy.example.com:8080',
    httpsProxy: 'http://trusted-https-proxy.example.com:8080',
    noProxy: 'trusted.internal',
    strictSsl: true,
  }
  const config = {
    configByUri: {
      '//project.example.com/': { '@': { authToken: 'project-token' } },
    },
    httpProxy: 'http://project-http-proxy.example.com:8080',
    httpsProxy: 'http://project-https-proxy.example.com:8080',
    noProxy: 'project.internal',
    packageManagerRegistries,
    packageManagerNetworkConfig,
    registries: projectRegistries,
    strictSsl: false,
    virtualStoreDirMaxLength: 120,
  } as unknown as Config
  const context = {
    rootProjectManifestDir: '/repo',
    wantedPackageManager: {
      fromDevEngines: true,
      name: 'pnpm',
      onFail: 'download',
      version: '9.3.0',
    },
  } as unknown as ConfigContext

  await expect(switchCliVersion(config, context)).rejects.toThrow('exit 0')

  expect(createStoreController).toHaveBeenCalledWith(expect.objectContaining({
    configByUri: packageManagerNetworkConfig.configByUri,
    httpProxy: packageManagerNetworkConfig.httpProxy,
    httpsProxy: packageManagerNetworkConfig.httpsProxy,
    noProxy: packageManagerNetworkConfig.noProxy,
    registries: packageManagerRegistries,
    strictSsl: packageManagerNetworkConfig.strictSsl,
  }))
  expect(resolvePackageManagerIntegrities).not.toHaveBeenCalled()
  expect(installPnpmToStore).toHaveBeenCalledWith('9.3.0', expect.objectContaining({
    registries: packageManagerRegistries,
  }))
  expect(installPnpmToStore).not.toHaveBeenCalledWith('9.3.0', expect.objectContaining({
    registries: projectRegistries,
  }))

  exit.mockRestore()
})

test('switchCliVersion defaults package-manager registries to npmjs instead of project registries', async () => {
  const exit = jest.spyOn(process, 'exit').mockImplementation(((code?: string | number | null | undefined) => {
    throw new Error(`exit ${code ?? 0}`)
  }) as typeof process.exit)

  const projectRegistries = {
    '@pnpm': 'https://project-pnpm.example.com/',
    default: 'https://project.example.com/',
  }
  const config = {
    configByUri: {
      '//project.example.com/': { '@': { authToken: 'project-token' } },
    },
    httpProxy: 'http://project-http-proxy.example.com:8080',
    httpsProxy: 'http://project-https-proxy.example.com:8080',
    noProxy: 'project.internal',
    registries: projectRegistries,
    strictSsl: false,
    virtualStoreDirMaxLength: 120,
  } as unknown as Config
  const context = {
    rootProjectManifestDir: '/repo',
    wantedPackageManager: {
      fromDevEngines: true,
      name: 'pnpm',
      onFail: 'download',
      version: '9.3.0',
    },
  } as unknown as ConfigContext

  await expect(switchCliVersion(config, context)).rejects.toThrow('exit 0')

  expect(createStoreController).toHaveBeenCalledWith(expect.objectContaining({
    configByUri: {},
    httpProxy: undefined,
    httpsProxy: undefined,
    noProxy: undefined,
    registries: { default: 'https://registry.npmjs.org/' },
    strictSsl: undefined,
  }))
  expect(resolvePackageManagerIntegrities).not.toHaveBeenCalled()
  expect(installPnpmToStore).toHaveBeenCalledWith('9.3.0', expect.objectContaining({
    registries: { default: 'https://registry.npmjs.org/' },
  }))
  expect(installPnpmToStore).not.toHaveBeenCalledWith('9.3.0', expect.objectContaining({
    registries: projectRegistries,
  }))

  exit.mockRestore()
})

test('switchCliVersion installs from a registry-only package-manager lockfile without re-resolving', async () => {
  const exit = jest.spyOn(process, 'exit').mockImplementation(((code?: string | number | null | undefined) => {
    throw new Error(`exit ${code ?? 0}`)
  }) as typeof process.exit)

  await expect(switchCliVersion({
    registries: { default: 'https://registry.npmjs.org/' },
    virtualStoreDirMaxLength: 120,
  } as unknown as Config, {
    rootProjectManifestDir: '/repo',
    wantedPackageManager: {
      fromDevEngines: true,
      name: 'pnpm',
      onFail: 'download',
      version: '9.3.0',
    },
  } as unknown as ConfigContext)).rejects.toThrow('exit 0')

  expect(resolvePackageManagerIntegrities).not.toHaveBeenCalled()
  expect(installPnpmToStore).toHaveBeenCalledWith('9.3.0', expect.objectContaining({
    envLockfile,
  }))

  exit.mockRestore()
})

test('switchCliVersion accepts registry-only package-manager lockfiles with peer-suffixed snapshots', async () => {
  const exit = jest.spyOn(process, 'exit').mockImplementation(((code?: string | number | null | undefined) => {
    throw new Error(`exit ${code ?? 0}`)
  }) as typeof process.exit)
  const peerLockfile: EnvLockfile = {
    ...envLockfile,
    packages: {
      ...envLockfile.packages,
      '@pnpm/linux-x64@9.3.0': {
        resolution: {
          integrity: 'sha512-linux-x64',
        },
      },
      'peer-provider@1.0.0': {
        resolution: {
          integrity: 'sha512-peer-provider',
        },
      },
    },
    snapshots: {
      ...envLockfile.snapshots,
      '@pnpm/exe@9.3.0': {
        optionalDependencies: {
          '@pnpm/linux-x64': '9.3.0(peer-provider@1.0.0)',
        },
      },
      '@pnpm/linux-x64@9.3.0(peer-provider@1.0.0)': {
        dependencies: {
          'peer-provider': '1.0.0',
        },
        optional: true,
      },
      'peer-provider@1.0.0': {},
    },
  }

  readEnvLockfile.mockResolvedValueOnce(peerLockfile)

  await expect(switchCliVersion({
    registries: { default: 'https://registry.npmjs.org/' },
    virtualStoreDirMaxLength: 120,
  } as unknown as Config, {
    rootProjectManifestDir: '/repo',
    wantedPackageManager: {
      fromDevEngines: true,
      name: 'pnpm',
      onFail: 'download',
      version: '9.3.0',
    },
  } as unknown as ConfigContext)).rejects.toThrow('exit 0')

  expect(resolvePackageManagerIntegrities).not.toHaveBeenCalled()
  expect(installPnpmToStore).toHaveBeenCalledWith('9.3.0', expect.objectContaining({
    envLockfile: peerLockfile,
  }))

  exit.mockRestore()
})

test('switchCliVersion rejects package-manager lockfile resolutions with non-integrity fields', async () => {
  const poisonedLockfile: EnvLockfile = {
    ...envLockfile,
    packages: {
      ...envLockfile.packages,
      '@pnpm/linux-x64@9.3.0': {
        resolution: {
          integrity: 'sha512-poisoned',
          tarball: 'https://evil.example.com/pnpm-linux-x64.tgz',
        },
      },
    },
  }

  readEnvLockfile.mockResolvedValueOnce(poisonedLockfile)

  await expect(switchCliVersion({
    registries: { default: 'https://registry.npmjs.org/' },
    virtualStoreDirMaxLength: 120,
  } as unknown as Config, {
    rootProjectManifestDir: '/repo',
    wantedPackageManager: {
      fromDevEngines: true,
      name: 'pnpm',
      onFail: 'download',
      version: '9.3.0',
    },
  } as unknown as ConfigContext)).rejects.toThrow('integrity-only resolution')

  expect(resolvePackageManagerIntegrities).not.toHaveBeenCalled()
  expect(createStoreController).not.toHaveBeenCalled()
  expect(installPnpmToStore).not.toHaveBeenCalled()
  expect(spawnSync).not.toHaveBeenCalled()
})

test('switchCliVersion rejects package-manager lockfile dependencies with non-registry dep paths', async () => {
  const poisonedLockfile: EnvLockfile = {
    ...envLockfile,
    packages: {
      ...envLockfile.packages,
      'payload@file:../payload.tgz': {
        resolution: {
          integrity: 'sha512-payload',
        },
      },
    },
    snapshots: {
      ...envLockfile.snapshots,
      'pnpm@9.3.0': {
        dependencies: {
          payload: 'file:../payload.tgz',
        },
      },
      'payload@file:../payload.tgz': {},
    },
  }

  readEnvLockfile.mockResolvedValueOnce(poisonedLockfile)

  await expect(switchCliVersion({
    registries: { default: 'https://registry.npmjs.org/' },
    virtualStoreDirMaxLength: 120,
  } as unknown as Config, {
    rootProjectManifestDir: '/repo',
    wantedPackageManager: {
      fromDevEngines: true,
      name: 'pnpm',
      onFail: 'download',
      version: '9.3.0',
    },
  } as unknown as ConfigContext)).rejects.toThrow('registry package path')

  expect(resolvePackageManagerIntegrities).not.toHaveBeenCalled()
  expect(createStoreController).not.toHaveBeenCalled()
  expect(installPnpmToStore).not.toHaveBeenCalled()
  expect(spawnSync).not.toHaveBeenCalled()
})
