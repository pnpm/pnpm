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

// Mutable so a test can pretend the running pnpm is itself a broken release.
const mockPackageManager = { name: 'pnpm', version: '11.0.0' }
const actualCliMeta = await import('@pnpm/cli.meta')
jest.unstable_mockModule('@pnpm/cli.meta', () => ({
  ...actualCliMeta,
  packageManager: mockPackageManager,
}))
// Only the installer is faked. assertReleaseIsInstallable is the real one, so
// the list of broken releases stays in a single place and these tests exercise
// it rather than a copy.
const actualEnginePmCommands = await import('@pnpm/engine.pm.commands')
jest.unstable_mockModule('@pnpm/engine.pm.commands', () => ({
  ...actualEnginePmCommands,
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
  mockPackageManager.version = '11.0.0'
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
    registriesByScope: projectRegistries,
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
    registriesByScope: packageManagerRegistries,
    strictSsl: packageManagerNetworkConfig.strictSsl,
  }))
  expect(resolvePackageManagerIntegrities).not.toHaveBeenCalled()
  expect(installPnpmToStore).toHaveBeenCalledWith('9.3.0', expect.objectContaining({
    registriesByScope: packageManagerRegistries,
  }))
  expect(installPnpmToStore).not.toHaveBeenCalledWith('9.3.0', expect.objectContaining({
    registriesByScope: projectRegistries,
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
    registriesByScope: projectRegistries,
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
    registriesByScope: { default: 'https://registry.npmjs.org/' },
    strictSsl: undefined,
  }))
  expect(resolvePackageManagerIntegrities).not.toHaveBeenCalled()
  expect(installPnpmToStore).toHaveBeenCalledWith('9.3.0', expect.objectContaining({
    registriesByScope: { default: 'https://registry.npmjs.org/' },
  }))
  expect(installPnpmToStore).not.toHaveBeenCalledWith('9.3.0', expect.objectContaining({
    registriesByScope: projectRegistries,
  }))

  exit.mockRestore()
})

test('switchCliVersion installs from a registry-only package-manager lockfile without re-resolving', async () => {
  const exit = jest.spyOn(process, 'exit').mockImplementation(((code?: string | number | null | undefined) => {
    throw new Error(`exit ${code ?? 0}`)
  }) as typeof process.exit)

  await expect(switchCliVersion({
    registriesByScope: { default: 'https://registry.npmjs.org/' },
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
    registriesByScope: { default: 'https://registry.npmjs.org/' },
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

test('switchCliVersion discards package-manager lockfile resolutions with non-integrity fields and re-resolves them', async () => {
  // Deep clone: the discard mutates the lockfile it heals, and the shared
  // fixture must stay intact for the other tests.
  const poisonedLockfile = envLockfileFor('9.3.0')
  poisonedLockfile.packages['@pnpm/linux-x64@9.3.0' as keyof typeof poisonedLockfile.packages] = {
    resolution: {
      integrity: 'sha512-poisoned',
      tarball: 'https://evil.example.com/pnpm-linux-x64.tgz',
    },
  }

  readEnvLockfile.mockResolvedValueOnce(poisonedLockfile)

  const exit = jest.spyOn(process, 'exit').mockImplementation(((code?: string | number | null | undefined) => {
    throw new Error(`exit ${code ?? 0}`)
  }) as typeof process.exit)
  try {
    await expect(switchCliVersion({
      registriesByScope: { default: 'https://registry.npmjs.org/' },
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
  } finally {
    exit.mockRestore()
  }

  // The discarded entries must not survive into the re-resolution input.
  expect(poisonedLockfile.importers['.'].packageManagerDependencies).toBeUndefined()
  expect(resolvePackageManagerIntegrities).toHaveBeenCalledWith('9.3.0', expect.objectContaining({
    envLockfile: poisonedLockfile,
  }))
  // The install runs from the freshly resolved lockfile, not the poisoned one.
  expect(installPnpmToStore).toHaveBeenCalledWith('9.3.0', expect.objectContaining({
    envLockfile,
  }))
})

test('switchCliVersion discards package-manager lockfile dependencies with non-registry dep paths and re-resolves them', async () => {
  const poisonedLockfile = envLockfileFor('9.3.0')
  Object.assign(poisonedLockfile.packages, {
    'payload@file:../payload.tgz': {
      resolution: {
        integrity: 'sha512-payload',
      },
    },
  })
  Object.assign(poisonedLockfile.snapshots, {
    'pnpm@9.3.0': {
      dependencies: {
        payload: 'file:../payload.tgz',
      },
    },
    'payload@file:../payload.tgz': {},
  })

  readEnvLockfile.mockResolvedValueOnce(poisonedLockfile)

  const exit = jest.spyOn(process, 'exit').mockImplementation(((code?: string | number | null | undefined) => {
    throw new Error(`exit ${code ?? 0}`)
  }) as typeof process.exit)
  try {
    await expect(switchCliVersion({
      registriesByScope: { default: 'https://registry.npmjs.org/' },
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
  } finally {
    exit.mockRestore()
  }

  expect(resolvePackageManagerIntegrities).toHaveBeenCalledWith('9.3.0', expect.anything())
  expect(installPnpmToStore).toHaveBeenCalledWith('9.3.0', expect.objectContaining({
    envLockfile,
  }))
})

test('switchCliVersion rejects a package-manager lockfile that is still invalid after re-resolving', async () => {
  const poisonLinuxX64 = (lockfile: EnvLockfile) => {
    lockfile.packages['@pnpm/linux-x64@9.3.0' as keyof typeof lockfile.packages] = {
      resolution: {
        integrity: 'sha512-poisoned',
        tarball: 'https://evil.example.com/pnpm-linux-x64.tgz',
      },
    }
    return lockfile
  }

  readEnvLockfile.mockResolvedValueOnce(poisonLinuxX64(envLockfileFor('9.3.0')))
  resolvePackageManagerIntegrities.mockResolvedValueOnce(poisonLinuxX64(envLockfileFor('9.3.0')))

  await expect(switchCliVersion({
    registriesByScope: { default: 'https://registry.npmjs.org/' },
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

  expect(installPnpmToStore).not.toHaveBeenCalled()
  expect(spawnSync).not.toHaveBeenCalled()
})

test('refuses to switch to a broken release instead of failing inside the installer', async () => {
  const config = { rawConfig: {} } as unknown as Config
  const context = {
    rootProjectManifestDir: '/project',
    wantedPackageManager: { name: 'pnpm', version: '11.12.0', fromDevEngines: true, onFail: 'download' },
  } as unknown as ConfigContext
  readEnvLockfile.mockResolvedValue(envLockfileFor('11.12.0'))

  // Spied so that a regression, which would run the switch to completion and
  // reach exit(), fails the test instead of ending the worker.
  const exit = jest.spyOn(process, 'exit').mockImplementation((() => undefined) as never)
  try {
    await expect(switchCliVersion(config, context)).rejects.toThrow(/11\.12\.0 is a broken release/)
  } finally {
    exit.mockRestore()
  }

  expect(installPnpmToStore).not.toHaveBeenCalled()
})

// The refusal must not strand anyone: a developer whose pnpm *is* a broken
// release still needs it to run, or they have nothing to move off it with.
test('does not refuse a broken release that is already the running version', async () => {
  mockPackageManager.version = '11.12.0'
  const config = { rawConfig: {} } as unknown as Config
  const context = {
    rootProjectManifestDir: '/project',
    wantedPackageManager: { name: 'pnpm', version: '11.12.0', fromDevEngines: true, onFail: 'download' },
  } as unknown as ConfigContext
  readEnvLockfile.mockResolvedValue(envLockfileFor('11.12.0'))

  await expect(switchCliVersion(config, context)).resolves.toBeUndefined()

  expect(installPnpmToStore).not.toHaveBeenCalled()
})

test('still switches to a release that is not broken', async () => {
  const config = { rawConfig: {} } as unknown as Config
  const context = {
    rootProjectManifestDir: '/project',
    wantedPackageManager: { name: 'pnpm', version: '11.13.1', fromDevEngines: true, onFail: 'download' },
  } as unknown as ConfigContext
  readEnvLockfile.mockResolvedValue(envLockfileFor('11.13.1'))

  const exit = jest.spyOn(process, 'exit').mockImplementation((() => undefined) as never)
  try {
    await switchCliVersion(config, context)
  } finally {
    exit.mockRestore()
  }

  expect(installPnpmToStore).toHaveBeenCalledWith('11.13.1', expect.anything())
})

/** The fixture above, re-pointed at `version` — same shape, so it still passes the
 * registry-resolution assertion the switcher makes before installing. */
function envLockfileFor (version: string): EnvLockfile {
  return JSON.parse(JSON.stringify(envLockfile).replaceAll('9.3.0', version)) as EnvLockfile
}
