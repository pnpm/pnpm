import fs from 'fs'
import { prepare, prepareEmpty } from '@pnpm/prepare'
import { readModulesManifest } from '@pnpm/modules-yaml'
import { type WorkspaceManifest } from '@pnpm/workspace.read-manifest'
import { sync as writeYamlFile } from 'write-yaml-file'
import { execPnpm } from '../utils/index.js'

const describeOnLinuxOnly = process.platform === 'linux' ? describe : describe.skip

type CPU = 'arm64' | 'x64'
type LibC = 'glibc' | 'musl'
type OS = 'darwin' | 'linux' | 'win32'
type CLIOption = `--cpu=${CPU}` | `--libc=${LibC}` | `--os=${OS}`
interface WorkspaceConfig {
  cpu?: CPU[]
  libc?: LibC[]
  os?: OS[]
}
type Installed = string[]
type Skipped = string[]
type Case = [
  CLIOption[],
  WorkspaceConfig | undefined,
  Installed,
  Skipped
]

const TEST_CASES: Case[] = [
  [[], undefined, [
    'only-linux-x64-glibc',
    'only-linux-x64-musl',
  ], [
    'only-darwin-arm64',
    'only-darwin-x64',
    'only-linux-arm64-glibc',
    'only-linux-arm64-musl',
    'only-win32-arm64',
    'only-win32-x64',
  ]],

  [[], {
    os: ['win32'],
    cpu: ['arm64', 'x64'],
  }, [
    'only-win32-arm64',
    'only-win32-x64',
  ], [
    'only-darwin-arm64',
    'only-darwin-x64',
    'only-linux-arm64-glibc',
    'only-linux-arm64-musl',
    'only-linux-x64-glibc',
    'only-linux-x64-musl',
  ]],

  [[
    '--os=darwin',
    '--cpu=arm64',
    '--cpu=x64',
  ], undefined, [
    'only-darwin-arm64',
    'only-darwin-x64',
  ], [
    'only-linux-arm64-glibc',
    'only-linux-arm64-musl',
    'only-linux-x64-glibc',
    'only-linux-x64-musl',
    'only-win32-arm64',
    'only-win32-x64',
  ]],

  [[
    '--os=darwin',
    '--cpu=arm64',
    '--cpu=x64',
  ], {
    os: ['win32'],
    cpu: ['arm64', 'x64'],
  }, [
    'only-darwin-arm64',
    'only-darwin-x64',
  ], [
    'only-linux-arm64-glibc',
    'only-linux-arm64-musl',
    'only-linux-x64-glibc',
    'only-linux-x64-musl',
    'only-win32-arm64',
    'only-win32-x64',
  ]],
]

describeOnLinuxOnly('install with supportedArchitectures from CLI options and manifest.pnpm', () => {
  test.each(TEST_CASES)('%j on %j', async (cliOpts, workspaceConfig, installed, skipped) => {
    prepare({
      dependencies: {
        '@pnpm.e2e/support-different-architectures': '1.0.0',
      },
    })

    writeYamlFile('pnpm-workspace.yaml', {
      supportedArchitectures: workspaceConfig,
    } as WorkspaceManifest)

    await execPnpm([
      'install',
      '--reporter=append-only',
      ...cliOpts,
    ])

    const modulesManifest = await readModulesManifest('node_modules')
    expect(Object.keys(modulesManifest?.hoistedDependencies ?? {}).sort()).toStrictEqual(installed.map(name => `@pnpm.e2e/${name}@1.0.0`))
    expect(modulesManifest?.skipped.sort()).toStrictEqual(skipped.map(name => `@pnpm.e2e/${name}@1.0.0`))

    expect(fs.readdirSync('node_modules/.pnpm/node_modules/@pnpm.e2e/')).toStrictEqual(installed)
  })
})

describeOnLinuxOnly('add with supportedArchitectures from CLI options and manifest.pnpm', () => {
  test.each(TEST_CASES)('%j on %j', async (cliOpts, workspaceConfig, installed, skipped) => {
    prepareEmpty()

    writeYamlFile('pnpm-workspace.yaml', {
      supportedArchitectures: workspaceConfig,
    } as WorkspaceManifest)

    await execPnpm([
      'add',
      '--reporter=append-only',
      ...cliOpts,
      '@pnpm.e2e/support-different-architectures',
    ])

    const modulesManifest = await readModulesManifest('node_modules')
    expect(Object.keys(modulesManifest?.hoistedDependencies ?? {}).sort()).toStrictEqual(installed.map(name => `@pnpm.e2e/${name}@1.0.0`))
    expect(modulesManifest?.skipped.sort()).toStrictEqual(skipped.map(name => `@pnpm.e2e/${name}@1.0.0`))

    expect(fs.readdirSync('node_modules/.pnpm/node_modules/@pnpm.e2e/')).toStrictEqual(installed)
  })
})
