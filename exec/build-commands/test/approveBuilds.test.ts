import fs from 'fs'
import path from 'path'
import type { ApproveBuildsCommandOpts } from '@pnpm/exec.build-commands'
import type { RebuildCommandOpts } from '@pnpm/plugin-commands-rebuild'
import { prepare } from '@pnpm/prepare'
import { getConfig } from '@pnpm/config'
import { readModulesManifest } from '@pnpm/modules-yaml'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { jest } from '@jest/globals'
import { omit } from 'ramda'
import { tempDir } from '@pnpm/prepare-temp-dir'
import { writePackageSync } from 'write-package'
import { sync as readYamlFile } from 'read-yaml-file'
import { sync as writeYamlFile } from 'write-yaml-file'
import execa from 'execa'

jest.unstable_mockModule('enquirer', () => ({ default: { prompt: jest.fn() } }))
const { default: enquirer } = await import('enquirer')
const { approveBuilds } = await import('@pnpm/exec.build-commands')

const prompt = jest.mocked(enquirer.prompt)

const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}/`
const pnpmBin = path.join(import.meta.dirname, '../../../pnpm/bin/pnpm.mjs')

async function execPnpmInstall (): Promise<void> {
  await execa('node', [
    pnpmBin,
    'install',
    `--store-dir=${path.resolve('store')}`,
    `--cache-dir=${path.resolve('cache')}`,
    `--registry=${REGISTRY}`,
    '--config.strict-dep-builds=false',
    '--config.enable-global-virtual-store=false',
  ])
}

async function getApproveBuildsConfig () {
  const cliOptions = {
    argv: [],
    dir: process.cwd(),
    registry: `http://localhost:${REGISTRY_MOCK_PORT}`,
  }
  return {
    ...omit(['reporter'], (await getConfig({
      cliOptions,
      packageManager: { name: 'pnpm', version: '' },
    })).config),
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

  await approveBuilds.handler({ ...config, ...opts })
}

async function approveNoBuilds (opts?: ApproveBuildsOptions) {
  await execPnpmInstall()
  const config = await getApproveBuildsConfig()

  prompt.mockResolvedValueOnce({
    result: [],
  })

  await approveBuilds.handler({ ...config, ...opts })
}

test('approve selected build', async () => {
  prepare({
    dependencies: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': '1.0.0',
      '@pnpm.e2e/install-script-example': '*',
    },
  })

  await approveSomeBuilds()

  const workspaceManifest = readYamlFile<any>(path.resolve('pnpm-workspace.yaml')) // eslint-disable-line
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

  const manifest = readYamlFile<any>(path.resolve('pnpm-workspace.yaml')) // eslint-disable-line
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
  writeYamlFile(workspaceManifestFile, { packages: ['packages/*'] })

  const config = await getApproveBuildsConfig()
  prompt.mockResolvedValueOnce({
    result: [{ value: '@pnpm.e2e/pre-and-postinstall-scripts-example' }],
  })
  prompt.mockResolvedValueOnce({ build: true })
  await approveBuilds.handler({ ...config, workspaceDir, rootProjectManifestDir: workspaceDir })

  expect(readYamlFile(workspaceManifestFile)).toStrictEqual({
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
  writeYamlFile(workspaceManifestFile, { packages: ['packages/*'] })
  await approveSomeBuilds({ workspaceDir: temp, rootProjectManifestDir: temp })

  expect(readYamlFile(workspaceManifestFile)).toStrictEqual({
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
  await approveBuilds.handler({ ...config, all: true })

  expect(prompt).not.toHaveBeenCalled()

  const workspaceManifest = readYamlFile<any>(path.resolve('pnpm-workspace.yaml')) // eslint-disable-line
  expect(workspaceManifest.allowBuilds).toStrictEqual({
    '@pnpm.e2e/install-script-example': true,
    '@pnpm.e2e/pre-and-postinstall-scripts-example': true,
  })

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeTruthy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeTruthy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeTruthy()
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
  writeYamlFile(workspaceManifestFile, {
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
  })

  expect(readYamlFile(workspaceManifestFile)).toStrictEqual({
    packages: ['packages/*'],
    allowBuilds: {
      '@pnpm.e2e/install-script-example': false,
      '@pnpm.e2e/pre-and-postinstall-scripts-example': true,
      '@pnpm.e2e/test': false,
    },
  })
})
