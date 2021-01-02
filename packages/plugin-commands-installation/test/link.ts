import readYamlFile from 'read-yaml-file'
import { install, link } from '@pnpm/plugin-commands-installation'
import prepare, { preparePackages } from '@pnpm/prepare'
import assertProject, { isExecutable } from '@pnpm/assert-project'
import { copyFixture } from '@pnpm/test-fixtures'
import { DEFAULT_OPTS } from './utils'
import path = require('path')
import PATH = require('path-name')
import fs = require('mz/fs')
import writePkg = require('write-pkg')

test('linking multiple packages', async () => {
  const project = prepare()

  process.chdir('..')
  const globalDir = path.resolve('global')

  await writePkg('linked-foo', { name: 'linked-foo', version: '1.0.0' })
  await writePkg('linked-bar', { name: 'linked-bar', version: '1.0.0', dependencies: { 'is-positive': '1.0.0' } })
  await fs.writeFile('linked-bar/.npmrc', 'shamefully-hoist = true')

  process.chdir('linked-foo')

  console.log('linking linked-foo to global package')
  const linkOpts = {
    ...DEFAULT_OPTS,
    npmGlobalBinDir: path.join(globalDir, 'bin'),
    globalDir,
  }
  await link.handler({
    ...linkOpts,
    dir: process.cwd(),
  })

  process.chdir('..')
  process.chdir('project')

  await link.handler({
    ...linkOpts,
    dir: process.cwd(),
  }, ['linked-foo', '../linked-bar'])

  await project.has('linked-foo')
  await project.has('linked-bar')

  const modules = await readYamlFile<object>('../linked-bar/node_modules/.modules.yaml')
  expect(modules['hoistPattern']).toStrictEqual(['*']) // the linked package used its own configs during installation // eslint-disable-line @typescript-eslint/dot-notation
})

test('link global bin', async function () {
  prepare()
  process.chdir('..')

  const globalDir = path.resolve('global')
  const globalBin = path.join(globalDir, 'bin')
  const oldPath = process.env[PATH]
  process.env[PATH] = `${globalBin}${path.delimiter}${oldPath ?? ''}`
  await fs.mkdir(globalBin, { recursive: true })

  await writePkg('package-with-bin', { name: 'package-with-bin', version: '1.0.0', bin: 'bin.js' })
  await fs.writeFile('package-with-bin/bin.js', '#!/usr/bin/env node\nconsole.log(/hi/)\n', 'utf8')

  process.chdir('package-with-bin')

  await link.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    npmGlobalBinDir: globalBin,
    globalDir,
  })
  process.env[PATH] = oldPath

  await isExecutable((value) => expect(value).toBeTruthy(), path.join(globalBin, 'package-with-bin'))
})

test('relative link', async () => {
  const project = prepare({
    dependencies: {
      'hello-world-js-bin': '*',
    },
  })

  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgPath = path.resolve('..', linkedPkgName)

  await copyFixture(linkedPkgName, linkedPkgPath)
  await link.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    npmGlobalBinDir: '',
  }, [`../${linkedPkgName}`])

  await project.isExecutable('.bin/hello-world-js-bin')

  // The linked package has been installed successfully as well with bins linked
  // to node_modules/.bin
  const linkedProject = assertProject(linkedPkgPath)
  await linkedProject.isExecutable('.bin/cowsay')

  const wantedLockfile = await project.readLockfile()
  expect(wantedLockfile.dependencies['hello-world-js-bin']).toBe('link:../hello-world-js-bin') // link added to wanted lockfile
  expect(wantedLockfile.specifiers['hello-world-js-bin']).toBe('*') // specifier of linked dependency added to ${WANTED_LOCKFILE}

  const currentLockfile = await project.readCurrentLockfile()
  expect(currentLockfile.dependencies['hello-world-js-bin']).toBe('link:../hello-world-js-bin') // link added to wanted lockfile
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
    npmGlobalBinDir: '',
  }, ['../source'])

  await projects['source'].has('is-positive')
  await projects['source'].hasNot('is-negative')

  // --production should not have effect on the target
  await projects['target'].has('is-positive')
  await projects['target'].has('is-negative')
})