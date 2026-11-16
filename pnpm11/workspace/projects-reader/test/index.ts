import fs from 'node:fs'
import path from 'node:path'

import { afterEach, beforeEach, expect, jest, test } from '@jest/globals'
import { logger } from '@pnpm/logger'
import {
  findWorkspaceProjects,
  findWorkspaceProjectsNoCheck,
  findWorkspaceProjectsNoCheckSync,
  findWorkspaceProjectsSync,
} from '@pnpm/workspace.projects-reader'
import { readWorkspaceManifest, readWorkspaceManifestSync } from '@pnpm/workspace.workspace-manifest-reader'
import { temporaryDirectory } from 'tempy'

beforeEach(() => {
  jest.spyOn(logger, 'warn')
})

afterEach(() => {
  jest.mocked(logger.warn).mockRestore()
})

test('findWorkspaceProjectsNoCheck() skips engine checks', async () => {
  const fixturePath = path.join(import.meta.dirname, '__fixtures__/bad-engine')

  const workspaceManifest = await readWorkspaceManifest(fixturePath)
  if (workspaceManifest?.packages == null) {
    throw new Error(`Unexpected test setup failure. No pnpm-workspace.yaml packages were defined at ${fixturePath}`)
  }

  const pkgs = await findWorkspaceProjectsNoCheck(fixturePath, {
    patterns: workspaceManifest.packages,
  })
  expect(pkgs).toHaveLength(1)
  expect(pkgs[0].manifest.name).toBe('pkg')
})

test('findWorkspaceProjects() outputs warnings for non-root workspace project', async () => {
  const fixturePath = path.join(import.meta.dirname, '__fixtures__/warning-for-non-root-project')

  const workspaceManifest = await readWorkspaceManifest(fixturePath)
  if (workspaceManifest?.packages == null) {
    throw new Error(`Unexpected test setup failure. No pnpm-workspace.yaml packages were defined at ${fixturePath}`)
  }

  const pkgs = await findWorkspaceProjects(fixturePath, {
    patterns: workspaceManifest.packages,
    sharedWorkspaceLockfile: true,
  })
  expect(pkgs).toHaveLength(3)
  const barPath = path.join(fixturePath, 'packages/bar')
  expect(
    jest.mocked(logger.warn).mock.calls
      .sort((a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b)))
  ).toStrictEqual([
    [{ prefix: barPath, message: `The field "resolutions" was found in ${barPath}/package.json. This will not take effect. Configure dependency overrides in pnpm-workspace.yaml using the "overrides" field instead.` }],
  ])
})
test('findWorkspaceProjects() fails on an engine mismatch of a project on an unsupported platform', async () => {
  const workspaceDir = temporaryDirectory()
  const projectDir = path.join(workspaceDir, 'packages/app')
  fs.mkdirSync(projectDir, { recursive: true })
  fs.writeFileSync(path.join(workspaceDir, 'package.json'), JSON.stringify({ name: 'root' }))
  fs.writeFileSync(path.join(projectDir, 'package.json'), JSON.stringify({
    name: 'app',
    os: [`!${process.platform}`],
    engines: { node: '>=99.0.0' },
  }))

  await expect(findWorkspaceProjects(workspaceDir, {
    patterns: ['packages/*'],
    engineStrict: true,
    nodeVersion: '20.0.0',
  })).rejects.toMatchObject({ code: 'ERR_PNPM_UNSUPPORTED_ENGINE' })
})

test('findWorkspaceProjects() checks engines.node against the project runtime', async () => {
  const workspaceDir = temporaryDirectory()
  const projectDir = path.join(workspaceDir, 'packages/app')
  fs.mkdirSync(projectDir, { recursive: true })
  fs.writeFileSync(path.join(workspaceDir, 'package.json'), JSON.stringify({ name: 'root' }))
  fs.writeFileSync(path.join(projectDir, 'package.json'), JSON.stringify({
    name: 'app',
    engines: { node: '>=99.0.0' },
    devEngines: { runtime: { name: 'node', version: '99.0.0', onFail: 'download' } },
  }))

  await expect(findWorkspaceProjects(workspaceDir, {
    patterns: ['packages/*'],
    engineStrict: true,
    nodeVersion: '22.0.0',
    nodeVersionFromEnginesRuntime: true,
    runtimeOnFail: 'download',
  })).resolves.toHaveLength(2)
})

test('findWorkspaceProjects() applies runtimeOnFail when checking a project runtime', async () => {
  const workspaceDir = temporaryDirectory()
  const projectDir = path.join(workspaceDir, 'packages/app')
  fs.mkdirSync(projectDir, { recursive: true })
  fs.writeFileSync(path.join(workspaceDir, 'package.json'), JSON.stringify({ name: 'root' }))
  fs.writeFileSync(path.join(projectDir, 'package.json'), JSON.stringify({
    name: 'app',
    engines: { node: '>=99.0.0' },
    devEngines: { runtime: { name: 'node', version: '99.0.0' } },
  }))

  await expect(findWorkspaceProjects(workspaceDir, {
    patterns: ['packages/*'],
    engineStrict: true,
    nodeVersion: '22.0.0',
    nodeVersionFromEnginesRuntime: true,
    runtimeOnFail: 'download',
  })).resolves.toHaveLength(2)
})

test('findWorkspaceProjects() keeps an explicitly configured Node.js version above a project runtime', async () => {
  const workspaceDir = temporaryDirectory()
  const projectDir = path.join(workspaceDir, 'packages/app')
  fs.mkdirSync(projectDir, { recursive: true })
  fs.writeFileSync(path.join(workspaceDir, 'package.json'), JSON.stringify({ name: 'root' }))
  fs.writeFileSync(path.join(projectDir, 'package.json'), JSON.stringify({
    name: 'app',
    engines: { node: '>=99.0.0' },
    devEngines: { runtime: { name: 'node', version: '99.0.0', onFail: 'download' } },
  }))

  await expect(findWorkspaceProjects(workspaceDir, {
    patterns: ['packages/*'],
    engineStrict: true,
    nodeVersion: '22.0.0',
  })).rejects.toMatchObject({ code: 'ERR_PNPM_UNSUPPORTED_ENGINE' })
})

test('findWorkspaceProjects() uses the shared Node.js version when a project runtime is not downloaded', async () => {
  const workspaceDir = temporaryDirectory()
  const projectDir = path.join(workspaceDir, 'packages/app')
  fs.mkdirSync(projectDir, { recursive: true })
  fs.writeFileSync(path.join(workspaceDir, 'package.json'), JSON.stringify({ name: 'root' }))
  fs.writeFileSync(path.join(projectDir, 'package.json'), JSON.stringify({
    name: 'app',
    engines: { node: '>=99.0.0' },
    devEngines: { runtime: { name: 'node', version: '99.0.0' } },
  }))

  await expect(findWorkspaceProjects(workspaceDir, {
    patterns: ['packages/*'],
    engineStrict: true,
    nodeVersion: '22.0.0',
    nodeVersionFromEnginesRuntime: true,
  })).rejects.toMatchObject({ code: 'ERR_PNPM_UNSUPPORTED_ENGINE' })
})

test('findWorkspaceProjects() uses a locked runtime version for engine checks', async () => {
  const workspaceDir = temporaryDirectory()
  const projectDir = path.join(workspaceDir, 'packages/app')
  fs.mkdirSync(projectDir, { recursive: true })
  fs.writeFileSync(path.join(workspaceDir, 'package.json'), JSON.stringify({ name: 'root' }))
  fs.writeFileSync(path.join(projectDir, 'package.json'), JSON.stringify({
    name: 'app',
    engines: { node: '>=24.1.0' },
    devEngines: { runtime: { name: 'node', version: '^24.0.0', onFail: 'download' } },
  }))
  fs.writeFileSync(path.join(workspaceDir, 'pnpm-lock.yaml'), [
    "lockfileVersion: '9.0'",
    'importers:',
    '  .: {}',
    '  packages/app:',
    '    devDependencies:',
    '      node:',
    '        specifier: ^24.0.0',
    '        version: runtime:24.1.0',
  ].join('\n'))

  await expect(findWorkspaceProjects(workspaceDir, {
    patterns: ['packages/*'],
    engineStrict: true,
    nodeVersion: '22.0.0',
    nodeVersionFromEnginesRuntime: true,
  })).resolves.toHaveLength(2)
})

test('findWorkspaceProjects() ignores a locked runtime for an outdated declaration', async () => {
  const workspaceDir = temporaryDirectory()
  const projectDir = path.join(workspaceDir, 'packages/app')
  fs.mkdirSync(projectDir, { recursive: true })
  fs.writeFileSync(path.join(workspaceDir, 'package.json'), JSON.stringify({ name: 'root' }))
  fs.writeFileSync(path.join(projectDir, 'package.json'), JSON.stringify({
    name: 'app',
    engines: { node: '>=24.1.0' },
    devEngines: { runtime: { name: 'node', version: '^24.0.0', onFail: 'download' } },
  }))
  fs.writeFileSync(path.join(workspaceDir, 'pnpm-lock.yaml'), [
    "lockfileVersion: '9.0'",
    'importers:',
    '  .: {}',
    '  packages/app:',
    '    devDependencies:',
    '      node:',
    '        specifier: runtime:^23.0.0',
    '        version: runtime:24.1.0',
  ].join('\n'))

  await expect(findWorkspaceProjects(workspaceDir, {
    patterns: ['packages/*'],
    engineStrict: true,
    nodeVersion: '22.0.0',
    nodeVersionFromEnginesRuntime: true,
  })).rejects.toMatchObject({ code: 'ERR_PNPM_UNSUPPORTED_ENGINE' })
})

test('findWorkspaceProjects() reads project lockfiles when the workspace lockfile is disabled', async () => {
  const workspaceDir = temporaryDirectory()
  const projectDir = path.join(workspaceDir, 'packages/app')
  fs.mkdirSync(projectDir, { recursive: true })
  fs.writeFileSync(path.join(workspaceDir, 'package.json'), JSON.stringify({ name: 'root' }))
  fs.writeFileSync(path.join(projectDir, 'package.json'), JSON.stringify({
    name: 'app',
    engines: { node: '>=24.1.0' },
    devEngines: { runtime: { name: 'node', version: '^24.0.0', onFail: 'download' } },
  }))
  fs.writeFileSync(path.join(projectDir, 'pnpm-lock.yaml'), [
    "lockfileVersion: '9.0'",
    'importers:',
    '  .:',
    '    devDependencies:',
    '      node:',
    '        specifier: runtime:^24.0.0',
    '        version: runtime:24.1.0',
  ].join('\n'))

  await expect(findWorkspaceProjects(workspaceDir, {
    patterns: ['packages/*'],
    engineStrict: true,
    nodeVersion: '22.0.0',
    nodeVersionFromEnginesRuntime: true,
    sharedWorkspaceLockfile: false,
  })).resolves.toHaveLength(2)
})

test('findWorkspaceProjects() checks dev runtimes when production and development dependencies are included', async () => {
  const workspaceDir = temporaryDirectory()
  const projectDir = path.join(workspaceDir, 'packages/app')
  fs.mkdirSync(projectDir, { recursive: true })
  fs.writeFileSync(path.join(workspaceDir, 'package.json'), JSON.stringify({ name: 'root' }))
  fs.writeFileSync(path.join(projectDir, 'package.json'), JSON.stringify({
    name: 'app',
    engines: { node: '>=99.0.0' },
    devEngines: { runtime: { name: 'node', version: '99.0.0', onFail: 'download' } },
  }))

  await expect(findWorkspaceProjects(workspaceDir, {
    patterns: ['packages/*'],
    engineStrict: true,
    nodeVersion: '22.0.0',
    nodeVersionFromEnginesRuntime: true,
    production: true,
    dev: true,
  })).resolves.toHaveLength(2)
})

test('findWorkspaceProjects() ignores a dev runtime excluded by a production install', async () => {
  const workspaceDir = temporaryDirectory()
  const projectDir = path.join(workspaceDir, 'packages/app')
  fs.mkdirSync(projectDir, { recursive: true })
  fs.writeFileSync(path.join(workspaceDir, 'package.json'), JSON.stringify({ name: 'root' }))
  fs.writeFileSync(path.join(projectDir, 'package.json'), JSON.stringify({
    name: 'app',
    engines: { node: '>=99.0.0' },
    devEngines: { runtime: { name: 'node', version: '99.0.0', onFail: 'download' } },
  }))
  fs.writeFileSync(path.join(workspaceDir, 'pnpm-lock.yaml'), [
    "lockfileVersion: '9.0'",
    'importers:',
    '  .: {}',
    '  packages/app:',
    '    devDependencies:',
    '      node:',
    '        specifier: 99.0.0',
    '        version: runtime:99.0.0',
  ].join('\n'))

  await expect(findWorkspaceProjects(workspaceDir, {
    patterns: ['packages/*'],
    engineStrict: true,
    nodeVersion: process.version,
    nodeVersionFromEnginesRuntime: true,
    production: true,
  })).rejects.toMatchObject({ code: 'ERR_PNPM_UNSUPPORTED_ENGINE' })
})

test('findWorkspaceProjects() ignores runtime pins when runtimes are skipped', async () => {
  const workspaceDir = temporaryDirectory()
  const projectDir = path.join(workspaceDir, 'packages/app')
  fs.mkdirSync(projectDir, { recursive: true })
  fs.writeFileSync(path.join(workspaceDir, 'package.json'), JSON.stringify({ name: 'root' }))
  fs.writeFileSync(path.join(projectDir, 'package.json'), JSON.stringify({
    name: 'app',
    engines: { node: '>=99.0.0' },
    devEngines: { runtime: { name: 'node', version: '99.0.0', onFail: 'download' } },
  }))

  await expect(findWorkspaceProjects(workspaceDir, {
    patterns: ['packages/*'],
    engineStrict: true,
    nodeVersion: '99.0.0',
    nodeVersionFromEnginesRuntime: true,
    skipRuntimes: true,
  })).rejects.toMatchObject({ code: 'ERR_PNPM_UNSUPPORTED_ENGINE' })
})

test('findWorkspaceProjectsNoCheckSync() works synchronously', () => {
  const fixturePath = path.join(import.meta.dirname, '__fixtures__/bad-engine')
  const workspaceManifest = readWorkspaceManifestSync(fixturePath)
  const pkgs = findWorkspaceProjectsNoCheckSync(fixturePath, {
    patterns: workspaceManifest?.packages,
  })
  expect(pkgs).toHaveLength(1)
  expect(pkgs[0].manifest.name).toBe('pkg')
})

test('findWorkspaceProjectsSync() works synchronously', () => {
  const fixturePath = path.join(import.meta.dirname, '__fixtures__/warning-for-non-root-project')
  const workspaceManifest = readWorkspaceManifestSync(fixturePath)
  const pkgs = findWorkspaceProjectsSync(fixturePath, {
    patterns: workspaceManifest?.packages,
    sharedWorkspaceLockfile: true,
  })
  expect(pkgs).toHaveLength(3)
  const barPath = path.join(fixturePath, 'packages/bar')
  expect(
    jest.mocked(logger.warn).mock.calls
      .sort((a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b)))
  ).toStrictEqual([
    [{ prefix: barPath, message: `The field "resolutions" was found in ${barPath}/package.json. This will not take effect. Configure dependency overrides in pnpm-workspace.yaml using the "overrides" field instead.` }],
  ])
})

const customModulesDirFixture = path.join(import.meta.dirname, '__fixtures__/custom-modules-dir')

test.each([
  [{ modulesDir: 'vendor' }, ['app', 'nested-dep', 'root', 'vendored-lib']],
  [{ modulesDir: 'vendor/' }, ['app', 'nested-dep', 'root', 'vendored-lib']],
  [{ modulesDir: 'deps/nested' }, ['app', 'dep', 'lib-dep', 'root', 'vendored-lib']],
  [{ modulesDir: 'node_modules' }, ['app', 'dep', 'lib-dep', 'nested-dep', 'root', 'vendored-lib']],
  [{ modulesDir: '../vendor' }, ['app', 'dep', 'lib-dep', 'nested-dep', 'root', 'vendored-lib']],
  [{ modulesDir: '.' }, ['app', 'dep', 'lib-dep', 'nested-dep', 'root', 'vendored-lib']],
  [{ modulesDir: path.join(customModulesDirFixture, 'packages/app/vendor') }, ['app', 'lib-dep', 'nested-dep', 'root', 'vendored-lib']],
  [{ modulesDir: path.resolve(customModulesDirFixture, '../vendor') }, ['app', 'dep', 'lib-dep', 'nested-dep', 'root', 'vendored-lib']],
  [{ modulesDirsByProjectName: { app: 'deps/nested' } }, ['app', 'dep', 'lib-dep', 'root', 'vendored-lib']],
  [{ modulesDir: 'vendor', modulesDirsByProjectName: { app: 'deps/nested' } }, ['app', 'dep', 'root', 'vendored-lib']],
  [{ modulesDirsByProjectName: { 'vendored-lib': 'vendor' } }, ['app', 'dep', 'nested-dep', 'root', 'vendored-lib']],
])('findWorkspaceProjectsNoCheck() skips the modules directories in %o', async (modulesDirOpts, expectedNames) => {
  const opts = { patterns: ['**'], ...modulesDirOpts }

  const names = (projects: Array<{ manifest: { name?: string } }>) => projects.map(({ manifest }) => manifest.name).sort()
  expect(names(await findWorkspaceProjectsNoCheck(customModulesDirFixture, opts))).toStrictEqual(expectedNames)
  expect(names(findWorkspaceProjectsNoCheckSync(customModulesDirFixture, opts))).toStrictEqual(expectedNames)
})
