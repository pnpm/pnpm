import fs from 'node:fs'
import { createRequire } from 'node:module'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { install } from '@pnpm/installing.commands'
import { preparePackages } from '@pnpm/prepare'
import { deploy } from '@pnpm/releasing.commands'
import { filterProjectsBySelectorObjectsFromDir } from '@pnpm/workspace.projects-filter'
import { loadJsonFileSync } from 'load-json-file'

import { DEFAULT_OPTS } from './utils/index.js'

test.each(['shared', 'separate', 'legacy'].flatMap(mode => ['@pnpm.e2e/foo', 'aliased'].map(alias => [mode, alias])))('offline %s deploy reuses %s without metadata', async (mode, alias) => {
  preparePackages([{
    location: '.',
    package: {
      name: 'app',
      version: '1.0.0',
      dependencies: { [alias]: alias === 'aliased' ? 'npm:@pnpm.e2e/foo@^100.0.0' : '^100.0.0' },
    },
  }])
  fs.writeFileSync('pnpm-workspace.yaml', 'packages:\n  - .\n')
  const { allProjects, selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [{ namePattern: 'app' }])
  const opts = {
    ...DEFAULT_OPTS,
    allProjects,
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    ignoreScripts: true,
    lockfileDir: process.cwd(),
    selectedProjectsGraph,
    sharedWorkspaceLockfile: mode !== 'separate',
    storeDir: path.resolve('store'),
    trustLockfile: true,
    workspaceDir: process.cwd(),
  }
  await install.handler(opts)
  const lockfile = fs.readFileSync('pnpm-lock.yaml', 'utf8')
  fs.rmSync(opts.cacheDir, { recursive: true, force: true })
  await install.handler({ ...opts, offline: true, frozenLockfile: true })
  await deploy.handler({
    ...opts,
    forceLegacyDeploy: mode === 'legacy',
    frozenLockfile: true,
    minimumReleaseAge: 1440,
    offline: true,
  }, ['out'])
  expect(loadJsonFileSync<{ version: string }>(`out/node_modules/${alias}/package.json`).version).toBe('100.1.0')
  expect(fs.readFileSync('pnpm-lock.yaml', 'utf8')).toBe(lockfile)
})

test('trustLockfile does not bypass resolution of a changed dependency during deploy', async () => {
  preparePackages([{
    location: '.',
    package: { name: 'app', version: '1.0.0', dependencies: { '@pnpm.e2e/foo': '100.0.0' } },
  }])
  fs.writeFileSync('pnpm-workspace.yaml', 'packages:\n  - .\n')
  const { allProjects, selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [{ namePattern: 'app' }])
  const opts = {
    ...DEFAULT_OPTS,
    allProjects,
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    lockfileDir: process.cwd(),
    selectedProjectsGraph,
    sharedWorkspaceLockfile: true,
    storeDir: path.resolve('store'),
    trustLockfile: true,
    workspaceDir: process.cwd(),
  }
  await install.handler(opts)
  fs.rmSync(opts.cacheDir, { recursive: true, force: true })
  allProjects[0].manifest.dependencies!['@pnpm.e2e/foo'] = '^101.0.0'
  await expect(deploy.handler({ ...opts, forceLegacyDeploy: true, offline: true }, ['out'])).rejects.toMatchObject({
    code: 'ERR_PNPM_NO_OFFLINE_META',
  })
})

test('offline legacy deploy reuses registry dependencies of workspace projects', async () => {
  preparePackages([
    { location: '.', package: { name: 'root', version: '1.0.0', private: true } },
    { name: 'app', version: '1.0.0', dependencies: { lib: 'workspace:*' } },
    { name: 'lib', version: '1.0.0', dependencies: { '@pnpm.e2e/pkg-with-1-dep': '100.0.0' } },
  ])
  fs.writeFileSync('pnpm-workspace.yaml', 'packages:\n  - app\n  - lib\n')
  const { allProjects, selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [{ namePattern: 'app' }])
  const opts = {
    ...DEFAULT_OPTS,
    allProjects,
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    injectWorkspacePackages: false,
    lockfileDir: process.cwd(),
    selectedProjectsGraph,
    sharedWorkspaceLockfile: true,
    storeDir: path.resolve('store'),
    trustLockfile: true,
    workspaceDir: process.cwd(),
  }
  const seed = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [{ namePattern: '*' }])
  await install.handler({ ...opts, selectedProjectsGraph: seed.selectedProjectsGraph })
  fs.rmSync(opts.cacheDir, { recursive: true, force: true })
  await deploy.handler({ ...opts, forceLegacyDeploy: true, offline: true }, ['out'])
  fs.renameSync('lib', 'source-lib')
  const deployedRequire = createRequire(path.resolve('out/package.json'))
  const libRequire = createRequire(deployedRequire.resolve('lib/package.json'))
  expect(libRequire('@pnpm.e2e/pkg-with-1-dep/package.json').version).toBe('100.0.0')
})
