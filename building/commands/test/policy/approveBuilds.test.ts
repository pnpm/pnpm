import fs from 'node:fs'
import path from 'node:path'

import { jest } from '@jest/globals'
import type { ApproveBuildsCommandOpts, RebuildCommandOpts } from '@pnpm/building.commands'
import { getConfig } from '@pnpm/config.reader'
import { readModulesManifest } from '@pnpm/installing.modules-yaml'
import { prepare } from '@pnpm/prepare'
import { tempDir } from '@pnpm/prepare-temp-dir'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { safeExeca as execa } from 'execa'
import { omit } from 'ramda'
import { readYamlFileSync } from 'read-yaml-file'
import { writePackageSync } from 'write-package'
import { writeYamlFileSync } from 'write-yaml-file'

jest.unstable_mockModule('enquirer', () => ({ default: { prompt: jest.fn() } }))
const { default: enquirer } = await import('enquirer')
const { approveBuilds } = await import('@pnpm/building.commands')

const prompt = jest.mocked(enquirer.prompt)

const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}/`
const pnpmBin = path.join(import.meta.dirname, '../../../../pnpm/bin/pnpm.mjs')

async function execPnpmInstall (opts?: { enableGlobalVirtualStore?: boolean }): Promise<void> {
  await execa('node', [
    pnpmBin,
    'install',
    `--store-dir=${path.resolve('store')}`,
    `--cache-dir=${path.resolve('cache')}`,
    `--registry=${REGISTRY}`,
    '--config.strict-dep-builds=false',
    `--config.enable-global-virtual-store=${opts?.enableGlobalVirtualStore ?? false}`,
  ])
}

async function getApproveBuildsConfig () {
  const cliOptions = {
    argv: [],
    dir: process.cwd(),
    registry: `http://localhost:${REGISTRY_MOCK_PORT}`,
  }
  const { config, context } = await getConfig({
    cliOptions,
    packageManager: { name: 'pnpm', version: '' },
  })
  return {
    ...omit(['reporter'], config),
    ...context,
    storeDir: path.resolve('store'),
    cacheDir: path.resolve('cache'),
    pnpmfile: [], // this is only needed because the pnpmfile returned by getConfig is string | string[]
    enableGlobalVirtualStore: false,
  }
}

type ApproveBuildsOptions = Partial<ApproveBuildsCommandOpts & RebuildCommandOpts>

async function approveSomeBuilds (opts?: ApproveBuildsOptions) {
  await execPnpmInstall()
  const config = await getApproveBuildsConfig()

  prompt.mockResolvedValueOnce({
    result: [
      {
        value: '@pnpm.e2e/pre-and-postinstall-scripts-example',
      },
    ],
  })
  prompt.mockResolvedValueOnce({
    build: true,
  })

  await approveBuilds.handler({ ...config, ...opts }, [], {})
}

async function approveNoBuilds (opts?: ApproveBuildsOptions) {
  await execPnpmInstall()
  const config = await getApproveBuildsConfig()

  prompt.mockResolvedValueOnce({
    result: [],
  })

  await approveBuilds.handler({ ...config, ...opts }, [], {})
}

test('approve selected build', async () => {
  prepare({
    dependencies: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': '1.0.0',
      '@pnpm.e2e/install-script-example': '*',
    },
  })

  await approveSomeBuilds()

  const workspaceManifest = readYamlFileSync<any>(path.resolve('pnpm-workspace.yaml')) // eslint-disable-line
  expect(workspaceManifest.allowBuilds).toStrictEqual({
    '@pnpm.e2e/install-script-example': false,
    '@pnpm.e2e/pre-and-postinstall-scripts-example': true,
  })

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeTruthy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeTruthy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeFalsy()
})

test('approve no builds', async () => {
  prepare({
    dependencies: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': '1.0.0',
      '@pnpm.e2e/install-script-example': '*',
    },
  })

  await approveNoBuilds()

  const manifest = readYamlFileSync<any>(path.resolve('pnpm-workspace.yaml')) // eslint-disable-line
  // allowBuilds is now the unified setting
  expect(Object.keys(manifest.allowBuilds ?? {}).sort()).toStrictEqual([
    '@pnpm.e2e/install-script-example',
    '@pnpm.e2e/pre-and-postinstall-scripts-example',
  ])

  expect(fs.readdirSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example')).not.toContain('generated-by-preinstall.js')
  expect(fs.readdirSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example')).not.toContain('generated-by-postinstall.js')
  expect(fs.readdirSync('node_modules/@pnpm.e2e/install-script-example')).not.toContain('generated-by-install.js')

  // Covers https://github.com/pnpm/pnpm/issues/9296
  expect((await readModulesManifest('node_modules'))!.ignoredBuilds).toBeUndefined()
})

test("works when root project manifest doesn't exist in a workspace", async () => {
  tempDir()

  writePackageSync('workspace/packages/project', {
    dependencies: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': '1.0.0',
      '@pnpm.e2e/install-script-example': '*',
    },
  })

  // Install before writing the workspace manifest so the CLI doesn't
  // detect a workspace (matching the old install.handler() behaviour
  // where getConfig() didn't read allowBuilds from the manifest).
  process.chdir('workspace/packages/project')
  await execPnpmInstall()

  const workspaceDir = path.resolve('../..')
  const workspaceManifestFile = path.join(workspaceDir, 'pnpm-workspace.yaml')
  writeYamlFileSync(workspaceManifestFile, { packages: ['packages/*'] })

  const config = await getApproveBuildsConfig()
  prompt.mockResolvedValueOnce({
    result: [{ value: '@pnpm.e2e/pre-and-postinstall-scripts-example' }],
  })
  prompt.mockResolvedValueOnce({ build: true })
  await approveBuilds.handler({ ...config, workspaceDir, rootProjectManifestDir: workspaceDir }, [], {})

  expect(readYamlFileSync(workspaceManifestFile)).toStrictEqual({
    packages: ['packages/*'],
    allowBuilds: {
      '@pnpm.e2e/install-script-example': false,
      '@pnpm.e2e/pre-and-postinstall-scripts-example': true,
    },
  })
})

test('should approve builds with package.json that has no allowBuilds field defined', async () => {
  const temp = tempDir()

  prepare({
    dependencies: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': '1.0.0',
      '@pnpm.e2e/install-script-example': '*',
    },
  }, {
    tempDir: temp,
  })

  const workspaceManifestFile = path.join(temp, 'pnpm-workspace.yaml')
  writeYamlFileSync(workspaceManifestFile, { packages: ['packages/*'] })
  await approveSomeBuilds({ workspaceDir: temp, rootProjectManifestDir: temp })

  expect(readYamlFileSync(workspaceManifestFile)).toStrictEqual({
    packages: ['packages/*'],
    allowBuilds: {
      '@pnpm.e2e/install-script-example': false,
      '@pnpm.e2e/pre-and-postinstall-scripts-example': true,
    },
  })
})

test('approve all builds with --all flag', async () => {
  prepare({
    dependencies: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': '1.0.0',
      '@pnpm.e2e/install-script-example': '*',
    },
  })

  await execPnpmInstall()
  const config = await getApproveBuildsConfig()

  prompt.mockClear()
  await approveBuilds.handler({ ...config, all: true }, [], {})

  expect(prompt).not.toHaveBeenCalled()

  const workspaceManifest = readYamlFileSync<any>(path.resolve('pnpm-workspace.yaml')) // eslint-disable-line
  expect(workspaceManifest.allowBuilds).toStrictEqual({
    '@pnpm.e2e/install-script-example': true,
    '@pnpm.e2e/pre-and-postinstall-scripts-example': true,
  })

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeTruthy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeTruthy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeTruthy()
})

test('approve builds via positional arguments', async () => {
  prepare({
    dependencies: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': '1.0.0',
      '@pnpm.e2e/install-script-example': '*',
    },
  })

  await execPnpmInstall()
  const config = await getApproveBuildsConfig()

  prompt.mockClear()
  await approveBuilds.handler(config, ['@pnpm.e2e/pre-and-postinstall-scripts-example'], {})

  expect(prompt).not.toHaveBeenCalled()

  const workspaceManifest = readYamlFileSync<any>(path.resolve('pnpm-workspace.yaml')) // eslint-disable-line
  expect(workspaceManifest.allowBuilds).toStrictEqual({
    '@pnpm.e2e/pre-and-postinstall-scripts-example': true,
  })

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeTruthy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeTruthy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeFalsy()

  // Unmentioned package should still be in ignoredBuilds after rebuild
  const modulesManifestAfter = await readModulesManifest(path.resolve('node_modules'))
  expect(modulesManifestAfter?.ignoredBuilds).toBeDefined()
})

test('deny builds via !pkg positional arguments', async () => {
  prepare({
    dependencies: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': '1.0.0',
      '@pnpm.e2e/install-script-example': '*',
    },
  })

  await execPnpmInstall()
  const config = await getApproveBuildsConfig()

  prompt.mockClear()
  await approveBuilds.handler(config, [
    '@pnpm.e2e/pre-and-postinstall-scripts-example',
    '!@pnpm.e2e/install-script-example',
  ], {})

  expect(prompt).not.toHaveBeenCalled()

  const workspaceManifest = readYamlFileSync<any>(path.resolve('pnpm-workspace.yaml')) // eslint-disable-line
  expect(workspaceManifest.allowBuilds).toStrictEqual({
    '@pnpm.e2e/install-script-example': false,
    '@pnpm.e2e/pre-and-postinstall-scripts-example': true,
  })

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeTruthy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeFalsy()
})

test('deny-only via !pkg keeps other builds pending', async () => {
  prepare({
    dependencies: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': '1.0.0',
      '@pnpm.e2e/install-script-example': '*',
    },
  })

  await execPnpmInstall()
  const config = await getApproveBuildsConfig()

  prompt.mockClear()
  await approveBuilds.handler(config, [
    '!@pnpm.e2e/install-script-example',
  ], {})

  expect(prompt).not.toHaveBeenCalled()

  const workspaceManifest = readYamlFileSync<any>(path.resolve('pnpm-workspace.yaml')) // eslint-disable-line
  expect(workspaceManifest.allowBuilds).toStrictEqual({
    '@pnpm.e2e/install-script-example': false,
  })

  const modulesManifestAfter = await readModulesManifest(path.resolve('node_modules'))
  const ignoredNames = Array.from(modulesManifestAfter?.ignoredBuilds ?? []).map(String)
  // The denied package should be removed from ignoredBuilds
  expect(ignoredNames.some((dp) => dp.includes('install-script-example'))).toBe(false)
  // The other package should still be pending
  expect(ignoredNames.some((dp) => dp.includes('pre-and-postinstall-scripts-example'))).toBe(true)
})

test('positional arguments with unknown package throws error', async () => {
  prepare({
    dependencies: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': '1.0.0',
    },
  })

  await execPnpmInstall()
  const config = await getApproveBuildsConfig()

  await expect(
    approveBuilds.handler(config, ['@pnpm.e2e/nonexistent-package'], {})
  ).rejects.toThrow('not awaiting approval')
})

test('!pkg with unknown package throws error', async () => {
  prepare({
    dependencies: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': '1.0.0',
    },
  })

  await execPnpmInstall()
  const config = await getApproveBuildsConfig()

  await expect(
    approveBuilds.handler(config, ['!@pnpm.e2e/nonexistent-package'], {})
  ).rejects.toThrow('not awaiting approval')
})

test('contradictory arguments throw error', async () => {
  prepare({
    dependencies: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': '1.0.0',
    },
  })

  await execPnpmInstall()
  const config = await getApproveBuildsConfig()

  await expect(
    approveBuilds.handler(config, [
      '@pnpm.e2e/pre-and-postinstall-scripts-example',
      '!@pnpm.e2e/pre-and-postinstall-scripts-example',
    ], {})
  ).rejects.toThrow('both approved and denied')
})

test('--all with positional arguments throws error', async () => {
  prepare({
    dependencies: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': '1.0.0',
    },
  })

  await execPnpmInstall()
  const config = await getApproveBuildsConfig()

  await expect(
    approveBuilds.handler({ ...config, all: true }, ['@pnpm.e2e/pre-and-postinstall-scripts-example'], {})
  ).rejects.toThrow('Cannot use --all with positional arguments')
})

test('positional args preserve existing allowBuilds entries', async () => {
  const temp = tempDir()

  prepare({
    dependencies: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': '1.0.0',
      '@pnpm.e2e/install-script-example': '*',
    },
  }, {
    tempDir: temp,
  })

  const workspaceManifestFile = path.join(temp, 'pnpm-workspace.yaml')
  writeYamlFileSync(workspaceManifestFile, {
    packages: ['packages/*'],
    allowBuilds: {
      '@pnpm.e2e/existing-package': true,
    },
  })

  await execPnpmInstall()
  const config = await getApproveBuildsConfig()

  await approveBuilds.handler({
    ...config,
    workspaceDir: temp,
    rootProjectManifestDir: temp,
    allowBuilds: {
      '@pnpm.e2e/existing-package': true,
    },
  }, ['@pnpm.e2e/pre-and-postinstall-scripts-example'], {})

  const manifest = readYamlFileSync<any>(workspaceManifestFile) // eslint-disable-line
  expect(manifest.allowBuilds['@pnpm.e2e/existing-package']).toBe(true)
  expect(manifest.allowBuilds['@pnpm.e2e/pre-and-postinstall-scripts-example']).toBe(true)
  // install-script-example should NOT be touched
  expect(manifest.allowBuilds['@pnpm.e2e/install-script-example']).toBeUndefined()
})

test('should retain existing allowBuilds entries when approving builds', async () => {
  const temp = tempDir()

  prepare({
    dependencies: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': '1.0.0',
      '@pnpm.e2e/install-script-example': '*',
    },
  }, {
    tempDir: temp,
  })

  // Install before writing the workspace manifest with allowBuilds so the
  // CLI ignores all builds (matching the old install.handler() behaviour
  // where getConfig() didn't read allowBuilds from the manifest).
  await execPnpmInstall()

  const workspaceManifestFile = path.join(temp, 'pnpm-workspace.yaml')
  writeYamlFileSync(workspaceManifestFile, {
    packages: ['packages/*'],
    allowBuilds: {
      '@pnpm.e2e/test': false,
      '@pnpm.e2e/install-script-example': true,
    },
  })

  const config = await getApproveBuildsConfig()
  prompt.mockResolvedValueOnce({
    result: [{ value: '@pnpm.e2e/pre-and-postinstall-scripts-example' }],
  })
  prompt.mockResolvedValueOnce({ build: true })
  await approveBuilds.handler({
    ...config,
    workspaceDir: temp,
    rootProjectManifestDir: temp,
    allowBuilds: {
      '@pnpm.e2e/test': false,
      '@pnpm.e2e/install-script-example': true,
    },
  }, [], {})

  expect(readYamlFileSync(workspaceManifestFile)).toStrictEqual({
    packages: ['packages/*'],
    allowBuilds: {
      '@pnpm.e2e/install-script-example': false,
      '@pnpm.e2e/pre-and-postinstall-scripts-example': true,
      '@pnpm.e2e/test': false,
    },
  })
})

// Regression test for the global-install path: globalAdd invokes
// approve-builds with globalPkgDir set (so writeSettings updates the global
// pnpm-workspace.yaml) but without workspaceDir. If approve-builds were to
// treat globalPkgDir as a workspace, install.handler would recursively
// discover sibling install dirs as workspace projects and fail the
// frozen-lockfile check on those that don't have a matching pnpm-lock.yaml.
test('GVS approve-builds writes settings to globalPkgDir without scanning siblings', async () => {
  const temp = tempDir()

  prepare({
    dependencies: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': '1.0.0',
    },
  }, {
    tempDir: path.join(temp, 'project'),
  })

  // Sibling install dir with a package.json that has no matching
  // pnpm-lock.yaml — mimics a stale `@pnpm/exe` install dir left behind in
  // the global packages directory.
  fs.mkdirSync(path.join(temp, 'stale-install'))
  fs.writeFileSync(
    path.join(temp, 'stale-install/package.json'),
    JSON.stringify({ dependencies: { '@pnpm/exe': '11.0.0-rc.2' } })
  )

  await execPnpmInstall({ enableGlobalVirtualStore: true })

  const config = await getApproveBuildsConfig()
  prompt.mockResolvedValueOnce({
    result: [{ value: '@pnpm.e2e/pre-and-postinstall-scripts-example' }],
  })
  prompt.mockResolvedValueOnce({ build: true })

  // Match the global-install call site: settingsDir points at the global
  // packages dir (for writeSettings) but workspaceDir is not set, so install
  // doesn't scan globalPkgDir as a workspace.
  await approveBuilds.handler({
    ...omit(['workspaceDir', 'workspacePackagePatterns'], config),
    enableGlobalVirtualStore: true,
    settingsDir: temp,
    rootProjectManifestDir: process.cwd(),
  } as ApproveBuildsCommandOpts & RebuildCommandOpts, [], {})

  // writeSettings should have written allowBuilds to globalPkgDir's
  // pnpm-workspace.yaml, not to the project dir.
  const globalManifest = readYamlFileSync<any>(path.join(temp, 'pnpm-workspace.yaml')) // eslint-disable-line
  expect(globalManifest.allowBuilds?.['@pnpm.e2e/pre-and-postinstall-scripts-example']).toBe(true)
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeTruthy()
})
