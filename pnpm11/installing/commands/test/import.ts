/// <reference path="../../../__typings__/index.d.ts" />
import fs from 'node:fs/promises'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { assertProject } from '@pnpm/assert-project'
import { PnpmError } from '@pnpm/error'
import { importCommand } from '@pnpm/installing.commands'
import { createEnvLockfile, readEnvLockfile, readWantedLockfile, writeEnvLockfile } from '@pnpm/lockfile.fs'
import { prepare } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'
import { addDistTag, REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
import { temporaryDirectory } from 'tempy'

const f = fixtures(import.meta.dirname)

const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}`
const TMP = temporaryDirectory()

const DEFAULT_OPTS = {
  ca: undefined,
  cacheDir: path.join(TMP, 'cache'),
  cert: undefined,
  fetchRetries: 2,
  fetchRetryFactor: 90,
  fetchRetryMaxtimeout: 90,
  fetchRetryMintimeout: 10,
  httpsProxy: undefined,
  key: undefined,
  localAddress: undefined,
  lock: false,
  lockStaleDuration: 90,
  minimumReleaseAge: 0,
  networkConcurrency: 16,
  offline: false,
  preferWorkspacePackages: true,
  proxy: undefined,
  pnpmHomeDir: '',
  configByUri: {},
  registriesByScope: { default: REGISTRY },
  registry: REGISTRY,
  rootProjectManifestDir: '',
  storeDir: path.join(TMP, 'store'),
  strictSsl: false,
  userAgent: 'pnpm',
  userConfig: {},
  useRunningStoreServer: false,
  useStoreServer: false,
  virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
}

test('import from package-lock.json', async () => {
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })
  f.prepare('has-package-lock-json')

  await importCommand.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  }, [])

  const project = assertProject(process.cwd())
  const lockfile = project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0'])
  expect(lockfile.packages).not.toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0'])

  // node_modules is not created
  project.hasNot('@pnpm.e2e/dep-of-pkg-with-1-dep')
  project.hasNot('@pnpm.e2e/pkg-with-1-dep')
})

test('import preserves the project lockfile when lockfileDir points elsewhere', async () => {
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })
  f.prepare('has-package-lock-json')
  const dir = process.cwd()
  const lockfileDir = path.join(dir, 'lockfile')
  await fs.mkdir(lockfileDir)
  const projectLockfile = '# project lockfile must stay unchanged\n'
  await fs.writeFile(path.join(dir, 'pnpm-lock.yaml'), projectLockfile)

  await importCommand.handler({
    ...DEFAULT_OPTS,
    dir,
    lockfileDir,
  }, [])

  const lockfile = assertProject(lockfileDir).readLockfile()
  expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0'])
  expect(lockfile.packages).not.toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0'])
  expect(await fs.readFile(path.join(dir, 'pnpm-lock.yaml'), 'utf8')).toBe(projectLockfile)
})

test('import preserves the env document in an external lockfile', async () => {
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })
  f.prepare('has-package-lock-json')
  const dir = process.cwd()
  const lockfileDir = path.join(dir, 'lockfile')
  await fs.mkdir(lockfileDir)
  const envLockfile = createEnvLockfile()
  envLockfile.importers['.'].configDependencies['@pnpm.e2e/foo'] = { specifier: '1.0.0', version: '1.0.0' }
  await writeEnvLockfile(lockfileDir, envLockfile)

  await importCommand.handler({
    ...DEFAULT_OPTS,
    dir,
    lockfileDir,
  }, [])

  const lockfile = await readWantedLockfile(lockfileDir, { ignoreIncompatible: false })
  expect(lockfile?.packages).toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0'])
  expect(lockfile?.packages).not.toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0'])
  expect(await readEnvLockfile(lockfileDir)).toEqual(envLockfile)
})

test.each(['missing', 'malformed', 'unresolvable'])('failed import preserves the external lockfile with %s input', async (input) => {
  f.prepare('has-package-lock-json')
  const dir = process.cwd()
  const lockfileDir = path.join(dir, 'lockfile')
  await fs.mkdir(lockfileDir)
  const lockfilePath = path.join(lockfileDir, 'pnpm-lock.yaml')
  await fs.writeFile(lockfilePath, "lockfileVersion: '9.0'\nimporters: {}\n")
  const envLockfile = createEnvLockfile()
  envLockfile.importers['.'].configDependencies['@pnpm.e2e/foo'] = { specifier: '1.0.0', version: '1.0.0' }
  await writeEnvLockfile(lockfileDir, envLockfile)
  const originalLockfile = await fs.readFile(lockfilePath, 'utf8')
  if (input === 'missing') {
    await fs.unlink(path.join(dir, 'package-lock.json'))
  } else if (input === 'malformed') {
    await fs.writeFile(path.join(dir, 'package-lock.json'), '{')
  } else {
    await fs.writeFile(path.join(dir, 'package.json'), JSON.stringify({
      dependencies: { '@pnpm.e2e/hello-world-js-bin-parent': '99.99.99' },
    }))
  }

  await expect(importCommand.handler({ ...DEFAULT_OPTS, dir, lockfileDir }, [])).rejects.toThrow()

  expect(await fs.readFile(lockfilePath, 'utf8')).toBe(originalLockfile)
  expect(await fs.readdir(lockfileDir)).toEqual(['pnpm-lock.yaml'])
})

test.each([
  { failure: false, existingBranch: true },
  { failure: false, existingBranch: false },
  { failure: true, existingBranch: true },
  { failure: true, existingBranch: false },
])('import preserves the shared lockfile with branch lockfiles ($failure, $existingBranch)', async ({ failure, existingBranch }) => {
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })
  f.prepare('has-package-lock-json')
  const dir = process.cwd()
  const lockfileDir = path.join(dir, 'lockfile')
  await fs.mkdir(lockfileDir)
  const npmLockfilePath = path.join(dir, 'package-lock.json')
  const npmLockfile = await fs.readFile(npmLockfilePath, 'utf8')
  // The shared lockfile resolves the transitive dependency to a version the
  // package-lock.json does not, so a branch import reading it gets caught.
  await fs.writeFile(npmLockfilePath, JSON.stringify({
    lockfileVersion: 1,
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': {
        version: '100.0.0',
        dependencies: { '@pnpm.e2e/dep-of-pkg-with-1-dep': { version: '100.1.0' } },
      },
    },
  }))
  await importCommand.handler({ ...DEFAULT_OPTS, dir, lockfileDir }, [])
  await fs.writeFile(npmLockfilePath, npmLockfile)
  await fs.mkdir(path.join(dir, '.git'))
  await fs.writeFile(path.join(dir, '.git/HEAD'), 'ref: refs/heads/feature/import\n')
  const sharedLockfilePath = path.join(lockfileDir, 'pnpm-lock.yaml')
  const envLockfile = createEnvLockfile()
  envLockfile.importers['.'].configDependencies['@pnpm.e2e/foo'] = { specifier: '1.0.0', version: '1.0.0' }
  await writeEnvLockfile(lockfileDir, envLockfile)
  const sharedLockfile = await fs.readFile(sharedLockfilePath, 'utf8')
  expect(sharedLockfile).toContain('@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0')
  const branchLockfilePath = path.join(lockfileDir, 'pnpm-lock.feature!import.yaml')
  const branchLockfile = '# existing branch lockfile\n'
  if (existingBranch) {
    await fs.writeFile(branchLockfilePath, branchLockfile)
  }
  if (failure) {
    await fs.writeFile(path.join(dir, 'package.json'), JSON.stringify({
      dependencies: { '@pnpm.e2e/hello-world-js-bin-parent': '99.99.99' },
    }))
  }

  const result = importCommand.handler({ ...DEFAULT_OPTS, dir, lockfileDir, useGitBranchLockfile: true }, [])
  if (failure) {
    await expect(result).rejects.toThrow()
    if (existingBranch) {
      expect(await fs.readFile(branchLockfilePath, 'utf8')).toBe(branchLockfile)
    }
  } else {
    await result
    const lockfile = await readWantedLockfile(lockfileDir, { ignoreIncompatible: false, useGitBranchLockfile: true })
    expect(lockfile?.packages).toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0'])
    expect(lockfile?.packages).not.toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0'])
  }
  expect(await fs.readFile(sharedLockfilePath, 'utf8')).toBe(sharedLockfile)
  expect((await fs.readdir(lockfileDir)).sort()).toEqual(existingBranch || !failure
    ? ['pnpm-lock.feature!import.yaml', 'pnpm-lock.yaml']
    : ['pnpm-lock.yaml'])
})

test('import from yarn.lock', async () => {
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  f.prepare('has-yarn-lock')

  await importCommand.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  }, [])

  const project = assertProject(process.cwd())
  const lockfile = project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0'])
  expect(lockfile.packages).not.toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0'])

  // node_modules is not created
  project.hasNot('@pnpm.e2e/dep-of-pkg-with-1-dep')
  project.hasNot('@pnpm.e2e/pkg-with-1-dep')
})

test('import from yarn2 lock file', async () => {
  f.prepare('has-yarn2-lock')

  await importCommand.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  }, [])

  const project = assertProject(process.cwd())
  const lockfile = project.readLockfile()

  expect(lockfile.packages).toHaveProperty(['is-positive@1.0.0'])
  expect(lockfile.packages).toHaveProperty(['is-negative@1.0.0'])

  // node_modules is not created
  project.hasNot('balanced-match')
  project.hasNot('brace-expansion')
})

test('import from npm-shrinkwrap.json', async () => {
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  f.prepare('has-npm-shrinkwrap-json')

  await importCommand.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  }, [])

  const project = assertProject(process.cwd())
  const lockfile = project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0'])
  expect(lockfile.packages).not.toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0'])

  // node_modules is not created
  project.hasNot('@pnpm.e2e/dep-of-pkg-with-1-dep')
  project.hasNot('@pnpm.e2e/pkg-with-1-dep')
})

test('import fails when no lockfiles are found', async () => {
  prepare(undefined)

  await expect(
    importCommand.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    }, [])
  ).rejects.toThrow(
    new PnpmError('LOCKFILE_NOT_FOUND', 'No lockfile found')
  )
})

test('import from package-lock.json v3', async () => {
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })
  f.prepare('has-package-lock-v3-json')

  await importCommand.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  }, [])

  const project = assertProject(process.cwd())
  const lockfile = project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0'])
  expect(lockfile.packages).not.toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0'])

  // node_modules is not created
  project.hasNot('@pnpm.e2e/dep-of-pkg-with-1-dep')
  project.hasNot('@pnpm.e2e/pkg-with-1-dep')
})
