import fs from 'fs'
import path from 'path'
import { install } from '@pnpm/plugin-commands-installation'
import { type ApproveBuildsCommandOpts } from '@pnpm/exec.build-commands'
import { type RebuildCommandOpts } from '@pnpm/plugin-commands-rebuild'
import { prepare } from '@pnpm/prepare'
import { getConfig } from '@pnpm/config'
import { type Modules, readModulesManifest } from '@pnpm/modules-yaml'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { jest } from '@jest/globals'
import { omit } from 'ramda'
import { tempDir } from '@pnpm/prepare-temp-dir'
import { writePackageSync } from 'write-pkg'
import { sync as readYamlFile } from 'read-yaml-file'
import { sync as writeYamlFile } from 'write-yaml-file'

jest.unstable_mockModule('enquirer', () => ({ default: { prompt: jest.fn() } }))
const { default: enquirer } = await import('enquirer')
const { approveBuilds } = await import('@pnpm/exec.build-commands')

const prompt = jest.mocked(enquirer.prompt)

type _ApproveBuildsOptions = Partial<ApproveBuildsCommandOpts & RebuildCommandOpts>

async function approveSomeBuilds (opts?: _ApproveBuildsOptions) {
  const cliOptions = {
    argv: [],
    dir: process.cwd(),
    registry: `http://localhost:${REGISTRY_MOCK_PORT}`,
  }
  const config = {
    ...omit(['reporter'], (await getConfig({
      cliOptions,
      packageManager: { name: 'pnpm', version: '' },
    })).config),
    storeDir: path.resolve('store'),
    cacheDir: path.resolve('cache'),
    pnpmfile: [], // this is only needed because the pnpmfile returned by getConfig is string | string[]
    enableGlobalVirtualStore: false,
  }
  await install.handler({ ...config, argv: { original: [] } })

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

async function approveNoBuilds (opts?: _ApproveBuildsOptions) {
  const cliOptions = {
    argv: [],
    dir: process.cwd(),
    registry: `http://localhost:${REGISTRY_MOCK_PORT}`,
  }
  const config = {
    ...omit(['reporter'], (await getConfig({
      cliOptions,
      packageManager: { name: 'pnpm', version: '' },
    })).config),
    storeDir: path.resolve('store'),
    cacheDir: path.resolve('cache'),
    pnpmfile: [], // this is only needed because the pnpmfile returned by getConfig is string | string[]
  }
  await install.handler({ ...config, argv: { original: [] } })

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

  const manifest = readYamlFile<any>(path.resolve('pnpm-workspace.yaml')) // eslint-disable-line
  expect(manifest.onlyBuiltDependencies).toStrictEqual(['@pnpm.e2e/pre-and-postinstall-scripts-example'])
  expect(manifest.ignoredBuiltDependencies).toStrictEqual(['@pnpm.e2e/install-script-example'])

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
  expect(manifest.onlyBuiltDependencies).toBeUndefined()
  expect(manifest.ignoredBuiltDependencies?.sort()).toStrictEqual([
    '@pnpm.e2e/install-script-example',
    '@pnpm.e2e/pre-and-postinstall-scripts-example',
  ])

  expect(fs.readdirSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example')).not.toContain('generated-by-preinstall.js')
  expect(fs.readdirSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example')).not.toContain('generated-by-postinstall.js')
  expect(fs.readdirSync('node_modules/@pnpm.e2e/install-script-example')).not.toContain('generated-by-install.js')

  // Covers https://github.com/pnpm/pnpm/issues/9296
  expect(await readModulesManifest('node_modules')).not.toHaveProperty(['ignoredBuilds' satisfies keyof Modules])
})

test("works when root project manifest doesn't exist in a workspace", async () => {
  tempDir()

  writePackageSync('workspace/packages/project', {
    dependencies: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': '1.0.0',
      '@pnpm.e2e/install-script-example': '*',
    },
  })

  const workspaceDir = path.resolve('workspace')
  const workspaceManifestFile = path.join(workspaceDir, 'pnpm-workspace.yaml')
  writeYamlFile(workspaceManifestFile, { packages: ['packages/*'] })
  process.chdir('workspace/packages/project')
  await approveSomeBuilds({ workspaceDir, rootProjectManifestDir: workspaceDir })

  expect(readYamlFile(workspaceManifestFile)).toStrictEqual({
    packages: ['packages/*'],
    onlyBuiltDependencies: ['@pnpm.e2e/pre-and-postinstall-scripts-example'],
    ignoredBuiltDependencies: ['@pnpm.e2e/install-script-example'],
  })
})

test('should update onlyBuiltDependencies when pnpm-workspace.yaml exists with ignoredBuiltDependencies defined', async () => {
  const temp = tempDir()
  const rootProjectManifest = {
    dependencies: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': '1.0.0',
      '@pnpm.e2e/install-script-example': '*',
    },
  }

  prepare(rootProjectManifest, {
    tempDir: temp,
  })

  const workspaceManifestFile = path.join(temp, 'pnpm-workspace.yaml')
  writeYamlFile(workspaceManifestFile, {
    packages: ['packages/*'],
    ignoredBuiltDependencies: ['@pnpm.e2e/install-script-example'],
  })
  await approveSomeBuilds({ workspaceDir: temp, rootProjectManifestDir: temp, rootProjectManifest })

  expect(readYamlFile(workspaceManifestFile)).toStrictEqual({
    packages: ['packages/*'],
    ignoredBuiltDependencies: ['@pnpm.e2e/install-script-example'],
    onlyBuiltDependencies: ['@pnpm.e2e/pre-and-postinstall-scripts-example'],
  })
})

test('should approve builds with package.json that has no onlyBuiltDependencies and ignoredBuiltDependencies fields defined', async () => {
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
    onlyBuiltDependencies: ['@pnpm.e2e/pre-and-postinstall-scripts-example'],
    ignoredBuiltDependencies: ['@pnpm.e2e/install-script-example'],
  })
})
