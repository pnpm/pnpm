import fs from 'fs'
import { add, install } from '@pnpm/plugin-commands-installation'
import { prepare } from '@pnpm/prepare'
import { getIntegrity } from '@pnpm/registry-mock'
import { type ProjectManifest } from '@pnpm/types'
import { sync as rimraf } from '@zkochan/rimraf'
import { sync as loadJsonFile } from 'load-json-file'
import { DEFAULT_OPTS } from './utils'

test('configuration dependency is installed', async () => {
  const rootProjectManifest: ProjectManifest = {
    pnpm: {
      configDependencies: {
        '@pnpm.e2e/foo': `100.0.0+${getIntegrity('@pnpm.e2e/foo', '100.0.0')}`,
      },
    },
  }
  prepare(rootProjectManifest)

  await install.handler({
    ...DEFAULT_OPTS,
    configDependencies: rootProjectManifest.pnpm!.configDependencies,
    dir: process.cwd(),
    rootProjectManifest,
    rootProjectManifestDir: process.cwd(),
  })

  {
    const configDepManifest = loadJsonFile<{ name: string, version: string }>('node_modules/.pnpm-config/@pnpm.e2e/foo/package.json')
    expect(configDepManifest.name).toBe('@pnpm.e2e/foo')
    expect(configDepManifest.version).toBe('100.0.0')
  }

  // Dependency is updated
  rootProjectManifest.pnpm!.configDependencies!['@pnpm.e2e/foo'] = `100.1.0+${getIntegrity('@pnpm.e2e/foo', '100.1.0')}`

  await install.handler({
    ...DEFAULT_OPTS,
    configDependencies: rootProjectManifest.pnpm!.configDependencies,
    dir: process.cwd(),
    rootProjectManifest,
    rootProjectManifestDir: process.cwd(),
  })

  {
    const configDepManifest = loadJsonFile<{ name: string, version: string }>('node_modules/.pnpm-config/@pnpm.e2e/foo/package.json')
    expect(configDepManifest.name).toBe('@pnpm.e2e/foo')
    expect(configDepManifest.version).toBe('100.1.0')
  }

  // Dependency is removed
  rootProjectManifest.pnpm!.configDependencies = {}

  await install.handler({
    ...DEFAULT_OPTS,
    configDependencies: rootProjectManifest.pnpm!.configDependencies,
    dir: process.cwd(),
    rootProjectManifest,
    rootProjectManifestDir: process.cwd(),
  })

  expect(fs.existsSync('node_modules/.pnpm-config/@pnpm.e2e/foo/package.json')).toBeFalsy()
})

test('patch from configuration dependency is applied', async () => {
  const rootProjectManifest = {
    pnpm: {
      configDependencies: {
        '@pnpm.e2e/has-patch-for-foo': `1.0.0+${getIntegrity('@pnpm.e2e/has-patch-for-foo', '1.0.0')}`,
      },
      patchedDependencies: {
        '@pnpm.e2e/foo@100.0.0': 'node_modules/.pnpm-config/@pnpm.e2e/has-patch-for-foo/@pnpm.e2e__foo@100.0.0.patch',
      },
    },
  }
  prepare(rootProjectManifest)

  await add.handler({
    ...DEFAULT_OPTS,
    configDependencies: rootProjectManifest.pnpm!.configDependencies,
    dir: process.cwd(),
    rootProjectManifest,
    rootProjectManifestDir: process.cwd(),
  }, ['@pnpm.e2e/foo@100.0.0'])

  expect(fs.existsSync('node_modules/@pnpm.e2e/foo/index.js')).toBeTruthy()
})

test('installation fails if the checksum of the config dependency is invalid', async () => {
  const rootProjectManifest: ProjectManifest = {
    pnpm: {
      configDependencies: {
        '@pnpm.e2e/foo': '100.0.0+sha512-00000000000000000000000000000000000000000000000000000000000000000000000000000000000000==',
      },
    },
  }
  prepare(rootProjectManifest)

  await expect(install.handler({
    ...DEFAULT_OPTS,
    configDependencies: rootProjectManifest.pnpm!.configDependencies,
    dir: process.cwd(),
    rootProjectManifest,
    rootProjectManifestDir: process.cwd(),
  })).rejects.toThrow('Got unexpected checksum for')
})

test('installation fails if the config dependency does not have a checksum', async () => {
  const rootProjectManifest: ProjectManifest = {
    pnpm: {
      configDependencies: {
        '@pnpm.e2e/foo': '100.0.0',
      },
    },
  }
  prepare(rootProjectManifest)

  await expect(install.handler({
    ...DEFAULT_OPTS,
    configDependencies: rootProjectManifest.pnpm!.configDependencies,
    dir: process.cwd(),
    rootProjectManifest,
    rootProjectManifestDir: process.cwd(),
  })).rejects.toThrow("doesn't have an integrity checksum")
})

test('selectively allow scripts in some dependencies by onlyBuiltDependenciesFile', async () => {
  const rootProjectManifest = {
    pnpm: {
      configDependencies: {
        '@pnpm.e2e/build-allow-list': `1.0.0+${getIntegrity('@pnpm.e2e/build-allow-list', '1.0.0')}`,
      },
      onlyBuiltDependenciesFile: 'node_modules/.pnpm-config/@pnpm.e2e/build-allow-list/list.json',
    },
  }
  prepare(rootProjectManifest)

  await add.handler({
    ...DEFAULT_OPTS,
    configDependencies: rootProjectManifest.pnpm!.configDependencies,
    dir: process.cwd(),
    rootProjectManifest,
    rootProjectManifestDir: process.cwd(),
  }, ['@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0', '@pnpm.e2e/install-script-example'])

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeTruthy()

  rimraf('node_modules')

  await install.handler({
    ...DEFAULT_OPTS,
    configDependencies: rootProjectManifest.pnpm!.configDependencies,
    dir: process.cwd(),
    frozenLockfile: true,
    rootProjectManifest,
    rootProjectManifestDir: process.cwd(),
  })

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeTruthy()
})

test('selectively allow scripts in some dependencies by onlyBuiltDependenciesFile and onlyBuiltDependencies', async () => {
  const rootProjectManifest = {
    pnpm: {
      configDependencies: {
        '@pnpm.e2e/build-allow-list': `1.0.0+${getIntegrity('@pnpm.e2e/build-allow-list', '1.0.0')}`,
      },
      onlyBuiltDependenciesFile: 'node_modules/.pnpm-config/@pnpm.e2e/build-allow-list/list.json',
      onlyBuiltDependencies: ['@pnpm.e2e/pre-and-postinstall-scripts-example'],
    },
  }
  prepare(rootProjectManifest)

  await add.handler({
    ...DEFAULT_OPTS,
    configDependencies: rootProjectManifest.pnpm!.configDependencies,
    dir: process.cwd(),
    rootProjectManifest,
    rootProjectManifestDir: process.cwd(),
  }, ['@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0', '@pnpm.e2e/install-script-example'])

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeTruthy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeTruthy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeTruthy()

  rimraf('node_modules')

  await install.handler({
    ...DEFAULT_OPTS,
    configDependencies: rootProjectManifest.pnpm!.configDependencies,
    dir: process.cwd(),
    frozenLockfile: true,
    rootProjectManifest,
    rootProjectManifestDir: process.cwd(),
  })

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeTruthy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeTruthy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeTruthy()
})
