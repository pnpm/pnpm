import fs from 'fs'
import path from 'path'
import { prepare, preparePackages, prepareEmpty } from '@pnpm/prepare'
import { isExecutable, assertProject } from '@pnpm/assert-project'
import { fixtures } from '@pnpm/test-fixtures'
import PATH from 'path-name'
import { sync as readYamlFile } from 'read-yaml-file'
import { writePackageSync } from 'write-package'
import { jest } from '@jest/globals'
import { sync as writeYamlFile } from 'write-yaml-file'
import { DEFAULT_OPTS } from './utils/index.js'

const original = await import('@pnpm/logger')
jest.unstable_mockModule('@pnpm/logger', () => {
  const logger = {
    ...original.logger,
    warn: jest.fn(),
  }
  return {
    ...original,
    logger: Object.assign(() => logger, logger),
  }
})

const { logger } = await import('@pnpm/logger')
const { install, link } = await import('@pnpm/plugin-commands-installation')

const f = fixtures(import.meta.dirname)

test('linking multiple packages', async () => {
  const project = prepare()

  process.chdir('..')

  writePackageSync('linked-foo', { name: 'linked-foo', version: '1.0.0' })
  writePackageSync('linked-bar', { name: 'linked-bar', version: '1.0.0', dependencies: { 'is-positive': '1.0.0' } })
  fs.writeFileSync('linked-bar/.npmrc', 'shamefully-hoist = true')

  process.chdir('project')

  await link.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    rootProjectManifestDir: process.cwd(),
  }, ['../linked-foo', '../linked-bar'])

  project.has('linked-foo')
  project.has('linked-bar')
})

test('link global bin', async function () {
  prepare()
  process.chdir('..')

  const globalDir = path.resolve('global')
  const globalBin = path.join(globalDir, 'bin')
  const oldPath = process.env[PATH]
  process.env[PATH] = `${globalBin}${path.delimiter}${oldPath ?? ''}`
  fs.mkdirSync(globalBin, { recursive: true })

  writePackageSync('package-with-bin', { name: 'package-with-bin', version: '1.0.0', bin: 'bin.js' })
  fs.writeFileSync('package-with-bin/bin.js', '#!/usr/bin/env node\nconsole.log(/hi/)\n', 'utf8')

  const pkgWithBinDir = path.resolve('package-with-bin')

  await link.handler({
    ...DEFAULT_OPTS,
    bin: globalBin,
    dir: globalDir,
    globalPkgDir: globalDir,
    rootProjectManifestDir: globalDir,
  }, [pkgWithBinDir])
  process.env[PATH] = oldPath

  isExecutable((value) => {
    expect(value).toBeTruthy()
  }, path.join(globalBin, 'package-with-bin'))
})

test('relative link', async () => {
  const project = prepare({
    dependencies: {
      '@pnpm.e2e/hello-world-js-bin': '*',
    },
  })

  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgPath = path.resolve('..', linkedPkgName)

  f.copy(linkedPkgName, linkedPkgPath)
  await link.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    globalPkgDir: '',
    rootProjectManifest: {
      dependencies: {
        '@pnpm.e2e/hello-world-js-bin': '*',
      },
    },
    rootProjectManifestDir: process.cwd(),
  }, [`../${linkedPkgName}`])

  project.isExecutable('.bin/hello-world-js-bin')

  const manifest = readYamlFile<{ overrides?: Record<string, string> }>('pnpm-workspace.yaml')
  expect(manifest.overrides?.['@pnpm.e2e/hello-world-js-bin']).toBe('link:../hello-world-js-bin')

  const wantedLockfile = project.readLockfile()
  expect(wantedLockfile.importers['.'].dependencies?.['@pnpm.e2e/hello-world-js-bin']).toStrictEqual({
    specifier: 'link:../hello-world-js-bin',
    version: 'link:../hello-world-js-bin',
  })

  const currentLockfile = project.readCurrentLockfile()
  expect(currentLockfile.importers['.'].dependencies?.['@pnpm.e2e/hello-world-js-bin'].version).toBe('link:../hello-world-js-bin') // link added to wanted lockfile
})

test('absolute link', async () => {
  const project = prepare({
    dependencies: {
      '@pnpm.e2e/hello-world-js-bin': '*',
    },
  })

  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgPath = path.resolve('..', linkedPkgName)

  f.copy(linkedPkgName, linkedPkgPath)
  await link.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    globalPkgDir: '',
    rootProjectManifestDir: process.cwd(),
    rootProjectManifest: {
      dependencies: {
        '@pnpm.e2e/hello-world-js-bin': '*',
      },
    },
  }, [linkedPkgPath])

  project.isExecutable('.bin/hello-world-js-bin')

  const wantedLockfile = project.readLockfile()
  expect(wantedLockfile.importers['.'].dependencies?.['@pnpm.e2e/hello-world-js-bin']).toStrictEqual({
    specifier: 'link:../hello-world-js-bin', // specifier of linked dependency added to ${WANTED_LOCKFILE}
    version: 'link:../hello-world-js-bin', // link added to wanted lockfile
  })

  const currentLockfile = project.readCurrentLockfile()
  expect(currentLockfile.importers['.'].dependencies?.['@pnpm.e2e/hello-world-js-bin'].version).toBe('link:../hello-world-js-bin') // link added to wanted lockfile
})

test('link --production', async () => {
  const targetManifest = {
    name: 'target',
    version: '1.0.0',

    dependencies: {
      'is-positive': '1.0.0',
    },
    devDependencies: {
      'is-negative': '1.0.0',
    },
  }
  const projects = preparePackages([
    targetManifest,
    {
      name: 'source',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
      },
      devDependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  process.chdir('target')

  await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })
  await link.handler({
    ...DEFAULT_OPTS,
    cliOptions: { production: true },
    dir: process.cwd(),
    globalPkgDir: '',
    rootProjectManifestDir: process.cwd(),
    rootProjectManifest: targetManifest,
  }, ['../source'])

  // --production should not have effect on the target
  projects['target'].has('is-positive')
  projects['target'].has('is-negative')
})

test('link fails if nothing is linked', async () => {
  prepare()

  await expect(
    link.handler({
      ...DEFAULT_OPTS,
      dir: '',
      globalPkgDir: '',
    }, [])
  ).rejects.toThrow(/You must provide a parameter/)
})

test('logger warns about peer dependencies when linking', async () => {
  prepare()

  process.chdir('..')

  writePackageSync('linked-with-peer-deps', {
    name: 'linked-with-peer-deps',
    version: '1.0.0',
    peerDependencies: {
      'some-peer-dependency': '1.0.0',
    },
  })

  process.chdir('project')

  await link.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    rootProjectManifestDir: process.cwd(),
  }, ['../linked-with-peer-deps'])

  expect(logger.warn).toHaveBeenCalledWith(expect.objectContaining({
    message: expect.stringContaining('has the following peerDependencies specified in its package.json'),
  }))

  jest.mocked(logger.warn).mockRestore()
})

test('logger should not warn about peer dependencies when it is an empty object', async () => {
  prepare()

  process.chdir('..')

  writePackageSync('linked-with-empty-peer-deps', {
    name: 'linked-with-empty-peer-deps',
    version: '1.0.0',
    peerDependencies: {},
  })

  process.chdir('project')

  await link.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    rootProjectManifestDir: process.cwd(),
  }, ['../linked-with-empty-peer-deps'])

  expect(logger.warn).not.toHaveBeenCalledWith(expect.objectContaining({
    message: expect.stringContaining('has the following peerDependencies specified in its package.json'),
  }))

  jest.mocked(logger.warn).mockRestore()
})

test('relative link from workspace package', async () => {
  prepareEmpty()

  const rootProjectManifest = {
    name: 'project',
    version: '1.0.0',
    dependencies: {
      '@pnpm.e2e/hello-world-js-bin': '*',
    },
  }
  writePackageSync('workspace/packages/project', rootProjectManifest)
  const workspaceDir = path.resolve('workspace')
  writeYamlFile(path.join(workspaceDir, 'pnpm-workspace.yaml'), { packages: ['packages/*'] })

  f.copy('hello-world-js-bin', 'hello-world-js-bin')

  const projectDir = path.resolve('workspace/packages/project')
  const helloWorldJsBinDir = path.resolve('hello-world-js-bin')

  process.chdir(projectDir)

  await link.handler({
    ...DEFAULT_OPTS,
    dedupeDirectDeps: false,
    dir: process.cwd(),
    globalPkgDir: '',
    lockfileDir: workspaceDir,
    rootProjectManifest,
    rootProjectManifestDir: workspaceDir,
    workspaceDir,
    workspacePackagePatterns: ['packages/*'],
  }, ['../../../hello-world-js-bin'])

  const manifest = readYamlFile<{ overrides?: Record<string, string> }>(path.join(workspaceDir, 'pnpm-workspace.yaml'))
  expect(manifest.overrides?.['@pnpm.e2e/hello-world-js-bin']).toBe('link:../hello-world-js-bin')

  const workspace = assertProject(workspaceDir)
  ;[workspace.readLockfile(), workspace.readCurrentLockfile()].forEach(lockfile => {
    expect(lockfile.importers['.'].dependencies?.['@pnpm.e2e/hello-world-js-bin'].version)
      .toBe('link:../hello-world-js-bin')
    expect(lockfile.importers['packages/project'].dependencies?.['@pnpm.e2e/hello-world-js-bin'].version)
      .toBe('link:../../../hello-world-js-bin')
  })

  const validateSymlink = (basePath: string) => {
    process.chdir(path.join(basePath, 'node_modules', '@pnpm.e2e'))
    expect(path.resolve(fs.readlinkSync('hello-world-js-bin'))).toBe(helloWorldJsBinDir)
  }

  validateSymlink(workspaceDir)
  validateSymlink(projectDir)
})
