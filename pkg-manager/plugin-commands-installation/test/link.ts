import fs from 'fs'
import path from 'path'
import { sync as readYamlFile } from 'read-yaml-file'
import { install, link } from '@pnpm/plugin-commands-installation'
import { prepare, preparePackages } from '@pnpm/prepare'
import { assertProject, isExecutable } from '@pnpm/assert-project'
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

  console.log('linking linked-foo to global package')
  const linkOpts = {
    ...DEFAULT_OPTS,
    bin: path.join(globalDir, 'bin'),
    dir: globalDir,
  }
  await link.handler({
    ...linkOpts,
  })

  process.chdir('..')
  process.chdir('project')

  await link.handler({
    ...linkOpts,
  }, ['linked-foo', '../linked-bar'])

  project.has('linked-foo')
  project.has('linked-bar')

  const modules = readYamlFile<any>('../linked-bar/node_modules/.modules.yaml') // eslint-disable-line @typescript-eslint/no-explicit-any
  expect(modules.hoistPattern).toStrictEqual(['*']) // the linked package used its own configs during installation // eslint-disable-line @typescript-eslint/dot-notation
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
    cliOptions: {
      global: true,
    },
    bin: globalBin,
    dir: globalDir,
  })
  process.env[PATH] = oldPath

  isExecutable((value) => {
    expect(value).toBeTruthy()
  }, path.join(globalBin, 'package-with-bin'))
})

test('link to global bin from the specified directory', async function () {
  prepare()
  process.chdir('..')

  const globalDir = path.resolve('global')
  const globalBin = path.join(globalDir, 'bin')
  const oldPath = process.env[PATH]
  process.env[PATH] = `${globalBin}${path.delimiter}${oldPath ?? ''}`
  fs.mkdirSync(globalBin, { recursive: true })

  await writePkg('./dir/package-with-bin-in-dir', { name: 'package-with-bin-in-dir', version: '1.0.0', bin: 'bin.js' })
  fs.writeFileSync('./dir/package-with-bin-in-dir/bin.js', '#!/usr/bin/env node\nconsole.log(/hi/)\n', 'utf8')

  await link.handler({
    ...DEFAULT_OPTS,
    cliOptions: {
      global: true,
      dir: path.resolve('./dir/package-with-bin-in-dir'),
    },
    bin: globalBin,
    dir: globalDir,
  })
  process.env[PATH] = oldPath

  isExecutable((value) => {
    expect(value).toBeTruthy()
  }, path.join(globalBin, 'package-with-bin-in-dir'))
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
    cliOptions: {
      global: true,
    },
    bin: globalBin,
    dir: globalDir,
  })

  process.chdir('..')
  const projectDir = path.resolve('./project')

  // link from global
  await link.handler({
    ...DEFAULT_OPTS,
    cliOptions: {
      global: true,
      dir: projectDir,
    },
    bin: globalBin,
    dir: globalDir,
    saveProd: true, // @pnpm/config sets this setting to true when global is true. This should probably be changed.
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
  }, [`../${linkedPkgName}`])

  project.isExecutable('.bin/hello-world-js-bin')

  // The linked package has been installed successfully as well with bins linked
  // to node_modules/.bin
  const linkedProject = assertProject(linkedPkgPath)
  linkedProject.isExecutable('.bin/cowsay')

  const wantedLockfile = project.readLockfile()
  expect(wantedLockfile.importers['.'].dependencies?.['@pnpm.e2e/hello-world-js-bin']).toStrictEqual({
    specifier: '*', // specifier of linked dependency added to ${WANTED_LOCKFILE}
    version: 'link:../hello-world-js-bin', // link added to wanted lockfile
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
  }, [linkedPkgPath])

  project.isExecutable('.bin/hello-world-js-bin')

  // The linked package has been installed successfully as well with bins linked
  // to node_modules/.bin
  const linkedProject = assertProject(linkedPkgPath)
  linkedProject.isExecutable('.bin/cowsay')

  const wantedLockfile = project.readLockfile()
  expect(wantedLockfile.importers['.'].dependencies?.['@pnpm.e2e/hello-world-js-bin']).toStrictEqual({
    specifier: '*', // specifier of linked dependency added to ${WANTED_LOCKFILE}
    version: 'link:../hello-world-js-bin', // link added to wanted lockfile
  })

  const currentLockfile = project.readCurrentLockfile()
  expect(currentLockfile.importers['.'].dependencies?.['@pnpm.e2e/hello-world-js-bin'].version).toBe('link:../hello-world-js-bin') // link added to wanted lockfile
})

test('link --production', async () => {
  const projects = preparePackages([
    {
      name: 'target',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
      },
      devDependencies: {
        'is-negative': '1.0.0',
      },
    },
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
  }, ['../source'])

  projects['source'].has('is-positive')
  projects['source'].hasNot('is-negative')

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

  const linkOpts = {
    ...DEFAULT_OPTS,
    bin: path.join(globalDir, 'bin'),
    dir: globalDir,
  }
  await link.handler({
    ...linkOpts,
  })

  process.chdir('..')
  process.chdir('project')

  await link.handler({
    ...linkOpts,
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

  const linkOpts = {
    ...DEFAULT_OPTS,
    bin: path.join(globalDir, 'bin'),
    dir: globalDir,
  }
  await link.handler({
    ...linkOpts,
  })

  process.chdir('..')
  process.chdir('project')

  await link.handler({
    ...linkOpts,
  }, ['linked-with-empty-peer-deps'])

  expect(warnMock).not.toHaveBeenCalledWith(expect.objectContaining({
    message: expect.stringContaining('has the following peerDependencies specified in its package.json'),
  }))

  warnMock.mockRestore()
})
