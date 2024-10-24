import fs from 'fs'
import path from 'path'
import { install, link } from '@pnpm/plugin-commands-installation'
import { prepare, preparePackages } from '@pnpm/prepare'
import { isExecutable } from '@pnpm/assert-project'
import { fixtures } from '@pnpm/test-fixtures'
import { logger } from '@pnpm/logger'
import { sync as loadJsonFile } from 'load-json-file'
import PATH from 'path-name'
import writePkg from 'write-pkg'
import { DEFAULT_OPTS } from './utils'

const f = fixtures(__dirname)

test('linking multiple packages', async () => {
  const project = prepare()

  process.chdir('..')
  const globalDir = path.resolve('global')

  await writePkg('linked-foo', { name: 'linked-foo', version: '1.0.0' })
  await writePkg('linked-bar', { name: 'linked-bar', version: '1.0.0', dependencies: { 'is-positive': '1.0.0' } })
  fs.writeFileSync('linked-bar/.npmrc', 'shamefully-hoist = true')

  process.chdir('linked-foo')

  // linking linked-foo to global package
  await link.handler({
    ...DEFAULT_OPTS,
    bin: path.join(globalDir, 'bin'),
    dir: globalDir,
    globalPkgDir: globalDir,
    rootProjectManifestDir: globalDir,
  })

  process.chdir('..')
  process.chdir('project')

  await link.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    globalPkgDir: globalDir,
    rootProjectManifestDir: process.cwd(),
  }, ['linked-foo', '../linked-bar'])

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

  await writePkg('package-with-bin', { name: 'package-with-bin', version: '1.0.0', bin: 'bin.js' })
  fs.writeFileSync('package-with-bin/bin.js', '#!/usr/bin/env node\nconsole.log(/hi/)\n', 'utf8')

  process.chdir('package-with-bin')

  await link.handler({
    ...DEFAULT_OPTS,
    bin: globalBin,
    dir: globalDir,
    globalPkgDir: globalDir,
    rootProjectManifestDir: globalDir,
  })
  process.env[PATH] = oldPath

  isExecutable((value) => {
    expect(value).toBeTruthy()
  }, path.join(globalBin, 'package-with-bin'))
})

test('link a global package to the specified directory', async function () {
  const project = prepare({ dependencies: { 'global-package-with-bin': '0.0.0' } })
  process.chdir('..')

  const globalDir = path.resolve('global')
  const globalBin = path.join(globalDir, 'bin')
  const oldPath = process.env[PATH]
  process.env[PATH] = `${globalBin}${path.delimiter}${oldPath ?? ''}`
  fs.mkdirSync(globalBin, { recursive: true })

  await writePkg('global-package-with-bin', { name: 'global-package-with-bin', version: '1.0.0', bin: 'bin.js' })
  fs.writeFileSync('global-package-with-bin/bin.js', '#!/usr/bin/env node\nconsole.log(/hi/)\n', 'utf8')

  process.chdir('global-package-with-bin')

  // link to global
  await link.handler({
    ...DEFAULT_OPTS,
    bin: globalBin,
    dir: globalDir,
    globalPkgDir: globalDir,
    rootProjectManifestDir: globalDir,
  })

  process.chdir('..')
  const projectDir = path.resolve('./project')

  // link from global
  await link.handler({
    ...DEFAULT_OPTS,
    // bin: globalBin,
    dir: projectDir,
    saveProd: true, // @pnpm/config sets this setting to true when global is true. This should probably be changed.
    globalPkgDir: globalDir,
    rootProjectManifest: { dependencies: { 'global-package-with-bin': '0.0.0' } },
    rootProjectManifestDir: projectDir,
  }, ['global-package-with-bin'])

  process.env[PATH] = oldPath

  const manifest = loadJsonFile<any>(path.join(projectDir, 'package.json')) // eslint-disable-line @typescript-eslint/no-explicit-any
  expect(manifest.dependencies).toStrictEqual({ 'global-package-with-bin': '0.0.0' })
  project.has('global-package-with-bin')
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

  const warnMock = jest.spyOn(logger, 'warn')

  process.chdir('..')
  const globalDir = path.resolve('global')

  await writePkg('linked-with-peer-deps', {
    name: 'linked-with-peer-deps',
    version: '1.0.0',
    peerDependencies: {
      'some-peer-dependency': '1.0.0',
    },
  })

  process.chdir('linked-with-peer-deps')

  await link.handler({
    ...DEFAULT_OPTS,
    bin: path.join(globalDir, 'bin'),
    dir: globalDir,
    globalPkgDir: globalDir,
    rootProjectManifestDir: globalDir,
  })

  process.chdir('..')
  process.chdir('project')

  await link.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    globalPkgDir: globalDir,
  }, ['linked-with-peer-deps'])

  expect(warnMock).toHaveBeenCalledWith(expect.objectContaining({
    message: expect.stringContaining('has the following peerDependencies specified in its package.json'),
  }))

  warnMock.mockRestore()
})

test('logger should not warn about peer dependencies when it is an empty object', async () => {
  prepare()

  const warnMock = jest.spyOn(logger, 'warn')

  process.chdir('..')
  const globalDir = path.resolve('global')

  await writePkg('linked-with-empty-peer-deps', {
    name: 'linked-with-empty-peer-deps',
    version: '1.0.0',
    peerDependencies: {},
  })

  process.chdir('linked-with-empty-peer-deps')

  await link.handler({
    ...DEFAULT_OPTS,
    globalPkgDir: '',
    bin: path.join(globalDir, 'bin'),
    dir: globalDir,
    rootProjectManifestDir: globalDir,
  })

  process.chdir('..')
  process.chdir('project')

  await link.handler({
    ...DEFAULT_OPTS,
    globalPkgDir: globalDir,
    dir: process.cwd(),
    rootProjectManifestDir: process.cwd(),
  }, ['linked-with-empty-peer-deps'])

  expect(warnMock).not.toHaveBeenCalledWith(expect.objectContaining({
    message: expect.stringContaining('has the following peerDependencies specified in its package.json'),
  }))

  warnMock.mockRestore()
})
