import assertProject, {isExecutable} from '@pnpm/assert-project'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import loadYamlFile = require('load-yaml-file')
const test = promisifyTape(tape)
import path = require('path')
import promisify = require('util.promisify')
import writePkg = require('write-pkg')
import {
  pathToLocalPkg,
  prepare,
  testDefaults,
  execPnpm,
 } from './utils'
import fs = require('mz/fs')
import isWindows = require('is-windows')
import ncpCB = require('ncp')

const ncp = promisify(ncpCB.ncp)

test('linking multiple packages', async (t: tape.Test) => {
  const project = prepare(t)

  process.chdir('..')
  process.env.NPM_CONFIG_PREFIX = path.resolve('global')

  await writePkg('linked-foo', {name: 'linked-foo', version: '1.0.0'})
  await writePkg('linked-bar', {name: 'linked-bar', version: '1.0.0', dependencies: {'is-positive': '1.0.0'}})
  await fs.writeFile('linked-bar/.npmrc', 'shamefully-flatten = true')

  process.chdir('linked-foo')

  t.comment('linking linked-foo to global package')
  await execPnpm('link')

  process.chdir('..')
  process.chdir('project')

  await execPnpm('link', 'linked-foo', '../linked-bar')

  project.has('linked-foo')
  project.has('linked-bar')

  const modules = await loadYamlFile('../linked-bar/node_modules/.modules.yaml')
  t.ok(modules['shamefullyFlatten'] === true, 'the linked package used its own configs during installation') // tslint:disable-line:no-string-literal
})

test('link global bin', async function (t: tape.Test) {
  prepare(t)
  process.chdir('..')

  const global = path.resolve('global')
  if (process.env.APPDATA) process.env.APPDATA = global
  process.env.NPM_CONFIG_PREFIX = global

  await writePkg('package-with-bin', {name: 'package-with-bin', version: '1.0.0', bin: 'bin.js'})
  await fs.writeFile('package-with-bin/bin.js', '#!/usr/bin/env node\nconsole.log(/hi/)\n', 'utf8')

  process.chdir('package-with-bin')

  await execPnpm('link')

  const globalBin = isWindows() ? path.join(global, 'npm') : path.join(global, 'bin')
  await isExecutable(t, path.join(globalBin, 'package-with-bin'))
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
  await execPnpm('link', `../${linkedPkgName}`)

  await project.isExecutable('.bin/hello-world-js-bin')

  // The linked package has been installed successfully as well with bins linked
  // to node_modules/.bin
  const linkedProject = assertProject(t, linkedPkgPath)
  await linkedProject.isExecutable('.bin/cowsay')

  const wantedShrinkwrap = await project.loadShrinkwrap()
  t.equal(wantedShrinkwrap.dependencies['hello-world-js-bin'], 'link:../hello-world-js-bin', 'link added to wanted shrinkwrap')
  t.equal(wantedShrinkwrap.specifiers['hello-world-js-bin'], '*', 'specifier of linked dependency added to shrinkwrap.yaml')

  const currentShrinkwrap = await project.loadCurrentShrinkwrap()
  t.equal(currentShrinkwrap.dependencies['hello-world-js-bin'], 'link:../hello-world-js-bin', 'link added to wanted shrinkwrap')
})
