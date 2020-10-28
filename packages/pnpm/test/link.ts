import { promisify } from 'util'
import promisifyTape from 'tape-promise'
import readYamlFile from 'read-yaml-file'
import prepare, { preparePackages } from '@pnpm/prepare'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import assertProject, { isExecutable } from '@pnpm/assert-project'
import {
  execPnpm,
  pathToLocalPkg,
} from './utils'
import path = require('path')
import fs = require('mz/fs')
import ncpCB = require('ncp')
import PATH = require('path-name')
import tape = require('tape')
import writePkg = require('write-pkg')

const ncp = promisify(ncpCB.ncp)
const test = promisifyTape(tape)

test('linking multiple packages', async (t: tape.Test) => {
  const project = prepare(t)

  process.chdir('..')

  const env = { NPM_CONFIG_PREFIX: path.resolve('global') }

  await writePkg('linked-foo', { name: 'linked-foo', version: '1.0.0' })
  await writePkg('linked-bar', { name: 'linked-bar', version: '1.0.0', dependencies: { 'is-positive': '1.0.0' } })
  await fs.writeFile('linked-bar/.npmrc', 'shamefully-flatten = true')

  process.chdir('linked-foo')

  t.comment('linking linked-foo to global package')
  await execPnpm(['link'], { env })

  process.chdir('..')
  process.chdir('project')

  await execPnpm(['link', 'linked-foo', '../linked-bar'], { env })

  await project.has('linked-foo')
  await project.has('linked-bar')

  const modules = await readYamlFile<object>('../linked-bar/node_modules/.modules.yaml')
  t.deepEqual(modules['hoistPattern'], ['*'], 'the linked package used its own configs during installation') // eslint-disable-line @typescript-eslint/dot-notation
})

test('link global bin', async function (t: tape.Test) {
  prepare(t)
  process.chdir('..')

  const global = path.resolve('global')
  const globalBin = path.join(global, 'nodejs')
  await fs.mkdir(globalBin, { recursive: true })
  const env = {
    NPM_CONFIG_PREFIX: global,
    [PATH]: `${globalBin}${path.delimiter}${process.env[PATH] ?? ''}`,
  }
  if (process.env.APPDATA) env['APPDATA'] = global

  await writePkg('package-with-bin', { name: 'package-with-bin', version: '1.0.0', bin: 'bin.js' })
  await fs.writeFile('package-with-bin/bin.js', '#!/usr/bin/env node\nconsole.log(/hi/)\n', 'utf8')

  process.chdir('package-with-bin')

  await execPnpm(['link'], { env })

  await isExecutable(t.ok, path.join(globalBin, 'package-with-bin'))
})

test('relative link', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      'hello-world-js-bin': '*',
    },
  })

  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgPath = path.resolve('..', linkedPkgName)

  await ncp(pathToLocalPkg(linkedPkgName), linkedPkgPath)
  await execPnpm(['link', `../${linkedPkgName}`])

  await project.isExecutable('.bin/hello-world-js-bin')

  // The linked package has been installed successfully as well with bins linked
  // to node_modules/.bin
  const linkedProject = assertProject(t, linkedPkgPath)
  await linkedProject.isExecutable('.bin/cowsay')

  const wantedLockfile = await project.readLockfile()
  t.equal(wantedLockfile.dependencies['hello-world-js-bin'], 'link:../hello-world-js-bin', 'link added to wanted lockfile')
  t.equal(wantedLockfile.specifiers['hello-world-js-bin'], '*', `specifier of linked dependency added to ${WANTED_LOCKFILE}`)

  const currentLockfile = await project.readCurrentLockfile()
  t.equal(currentLockfile.dependencies['hello-world-js-bin'], 'link:../hello-world-js-bin', 'link added to wanted lockfile')
})

test('link --production', async (t: tape.Test) => {
  const projects = preparePackages(t, [
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

  await execPnpm(['install'])
  await execPnpm(['link', '--production', '../source'])

  await projects['source'].has('is-positive')
  await projects['source'].hasNot('is-negative')

  // --production should not have effect on the target
  await projects['target'].has('is-positive')
  await projects['target'].has('is-negative')
})
