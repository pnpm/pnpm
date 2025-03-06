import fs from 'fs'
import path from 'path'
import * as enquirer from 'enquirer'
import { approveBuilds } from '@pnpm/exec.build-commands'
import { install } from '@pnpm/plugin-commands-installation'
import { prepare } from '@pnpm/prepare'
import { type ProjectManifest } from '@pnpm/types'
import { getConfig } from '@pnpm/config'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { sync as loadJsonFile } from 'load-json-file'
import omit from 'ramda/src/omit'
import { tempDir } from '@pnpm/prepare-temp-dir'
import writePkg from 'write-pkg'
import { sync as readYamlFile } from 'read-yaml-file'
import { sync as writeYamlFile } from 'write-yaml-file'

jest.mock('enquirer', () => ({ prompt: jest.fn() }))

// eslint-disable-next-line
const prompt = enquirer.prompt as any

const runApproveBuilds = async (opts = {}) => {
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

test('approve selected build', async () => {
  prepare({
    dependencies: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': '1.0.0',
      '@pnpm.e2e/install-script-example': '*',
    },
  })

  await runApproveBuilds()

  const manifest = loadJsonFile<ProjectManifest>(path.resolve('package.json'))
  expect(manifest.pnpm?.onlyBuiltDependencies).toStrictEqual(['@pnpm.e2e/pre-and-postinstall-scripts-example'])
  expect(manifest.pnpm?.ignoredBuiltDependencies).toStrictEqual(['@pnpm.e2e/install-script-example'])

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeTruthy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeTruthy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeFalsy()
})

test("works when root project manifest doesn't exist in a workspace", async () => {
  tempDir()

  await writePkg('workspace/packages/project', {
    dependencies: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': '1.0.0',
      '@pnpm.e2e/install-script-example': '*',
    },
  })

  const workspaceDir = path.resolve('workspace')
  const workspaceManifestFile = path.join(workspaceDir, 'pnpm-workspace.yaml')
  writeYamlFile(workspaceManifestFile, { packages: ['packages/*'] })
  process.chdir('workspace/packages/project')
  await runApproveBuilds({ workspaceDir, rootProjectManifestDir: workspaceDir })

  expect(readYamlFile(workspaceManifestFile)).toStrictEqual({
    packages: ['packages/*'],
    onlyBuiltDependencies: ['@pnpm.e2e/pre-and-postinstall-scripts-example'],
    ignoredBuiltDependencies: ['@pnpm.e2e/install-script-example'],
  })
})
