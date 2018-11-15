import { isExecutable } from '@pnpm/assert-project'
import prepare from '@pnpm/prepare'
import loadYamlFile = require('load-yaml-file')
import fs = require('mz/fs')
import ncpCB = require('ncp')
import path = require('path')
import exists = require('path-exists')
import readPkg = require('read-pkg')
import sinon = require('sinon')
import {
  install,
  installPkgs,
  link,
  linkFromGlobal,
  linkToGlobal,
  RootLog,
} from 'supi'
import symlink from 'symlink-dir'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import promisify = require('util.promisify')
import writeJsonFile from 'write-json-file'
import {
  pathToLocalPkg,
  testDefaults,
} from './utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)
const ncp = promisify(ncpCB.ncp)

test('relative link', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      'hello-world-js-bin': '*',
    },
  })

  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgPath = path.resolve('..', linkedPkgName)

  await ncp(pathToLocalPkg(linkedPkgName), linkedPkgPath)
  await link([`../${linkedPkgName}`], path.join(process.cwd(), 'node_modules'), await testDefaults())

  await project.isExecutable('.bin/hello-world-js-bin')

  const wantedShrinkwrap = await project.loadShrinkwrap()
  t.equal(wantedShrinkwrap.dependencies['hello-world-js-bin'], 'link:../hello-world-js-bin', 'link added to wanted shrinkwrap')
  t.equal(wantedShrinkwrap.specifiers['hello-world-js-bin'], '*', 'specifier of linked dependency added to shrinkwrap.yaml')

  const currentShrinkwrap = await project.loadCurrentShrinkwrap()
  t.equal(currentShrinkwrap.dependencies['hello-world-js-bin'], 'link:../hello-world-js-bin', 'link added to wanted shrinkwrap')
})

test('relative link is linked by the name of the alias', async (t: tape.Test) => {
  const linkedPkgName = 'hello-world-js-bin'

  const project = prepare(t, {
    dependencies: {
      hello: `link:../${linkedPkgName}`,
    },
  })

  const linkedPkgPath = path.resolve('..', linkedPkgName)

  await ncp(pathToLocalPkg(linkedPkgName), linkedPkgPath)
  await install(await testDefaults())

  await project.isExecutable('.bin/hello-world-js-bin')

  await project.has('hello')

  const wantedShrinkwrap = await project.loadShrinkwrap()
  t.deepEqual(wantedShrinkwrap.dependencies, {
    hello: 'link:../hello-world-js-bin',
  }, 'link added to wanted shrinkwrap with correct alias')
})

test('relative link is not rewritten by argumentless install', async (t: tape.Test) => {
  const project = prepare(t)

  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgPath = path.resolve('..', linkedPkgName)

  const reporter = sinon.spy()
  const opts = await testDefaults()

  await ncp(pathToLocalPkg(linkedPkgName), linkedPkgPath)
  await link([linkedPkgPath], path.join(process.cwd(), 'node_modules'), { ...opts, reporter }) // tslint:disable-line:no-any

  t.ok(reporter.calledWithMatch({
    added: {
      dependencyType: undefined,
      linkedFrom: linkedPkgPath,
      name: 'hello-world-js-bin',
      realName: 'hello-world-js-bin',
      version: '1.0.0',
    },
    level: 'debug',
    name: 'pnpm:root',
    prefix: process.cwd(),
  } as RootLog), 'linked root dependency logged')

  await install(opts)

  t.ok(project.requireModule('hello-world-js-bin/package.json').isLocal)

  const wantedShrinkwrap = await project.loadShrinkwrap()
  t.equal(wantedShrinkwrap.dependencies['hello-world-js-bin'], 'link:../hello-world-js-bin', 'link still in wanted shrinkwrap')

  const currentShrinkwrap = await project.loadCurrentShrinkwrap()
  t.equal(currentShrinkwrap.dependencies['hello-world-js-bin'], 'link:../hello-world-js-bin', 'link still in current shrinkwrap')
})

test('relative link is rewritten by named installation to regular dependency', async (t: tape.Test) => {
  const project = prepare(t)

  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgPath = path.resolve('..', linkedPkgName)

  const reporter = sinon.spy()
  const opts = await testDefaults()

  await ncp(pathToLocalPkg(linkedPkgName), linkedPkgPath)
  await link([linkedPkgPath], path.join(process.cwd(), 'node_modules'), { ...opts, reporter }) // tslint:disable-line:no-any

  t.ok(reporter.calledWithMatch({
    added: {
      dependencyType: undefined,
      linkedFrom: linkedPkgPath,
      name: 'hello-world-js-bin',
      realName: 'hello-world-js-bin',
      version: '1.0.0',
    },
    level: 'debug',
    name: 'pnpm:root',
    prefix: process.cwd(),
  } as RootLog), 'linked root dependency logged')

  await installPkgs(['hello-world-js-bin'], opts)

  const pkg = await readPkg()

  t.deepEqual(pkg.dependencies, { 'hello-world-js-bin': '^1.0.0' })

  t.notOk(project.requireModule('hello-world-js-bin/package.json').isLocal)

  const wantedShrinkwrap = await project.loadShrinkwrap()
  t.equal(wantedShrinkwrap.dependencies['hello-world-js-bin'], '1.0.0', 'link is not in wanted shrinkwrap anymore')

  const currentShrinkwrap = await project.loadCurrentShrinkwrap()
  t.equal(currentShrinkwrap.dependencies['hello-world-js-bin'], '1.0.0', 'link is not in current shrinkwrap anymore')
})

test('global link', async (t: tape.Test) => {
  const project = prepare(t)
  const projectPath = process.cwd()

  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgPath = path.resolve('..', linkedPkgName)

  await ncp(pathToLocalPkg(linkedPkgName), linkedPkgPath)

  const opts = await testDefaults()

  process.chdir(linkedPkgPath)
  const globalPrefix = path.resolve('..', 'global')
  const globalBin = path.resolve('..', 'global', 'bin')
  await linkToGlobal(process.cwd(), { ...opts, globalPrefix, globalBin }) // tslint:disable-line:no-any

  await isExecutable(t, path.join(globalBin, 'hello-world-js-bin'))

  // bins of dependencies should not be linked, see issue https://github.com/pnpm/pnpm/issues/905
  t.notOk(await exists(path.join(globalBin, 'cowsay')), 'cowsay not linked')
  t.notOk(await exists(path.join(globalBin, 'cowthink')), 'cowthink not linked')

  process.chdir(projectPath)

  await linkFromGlobal([linkedPkgName], process.cwd(), { ...opts, globalPrefix }) // tslint:disable-line:no-any

  await project.isExecutable('.bin/hello-world-js-bin')
})

test('failed linking should not create empty folder', async (t: tape.Test) => {
  prepare(t)

  const globalPrefix = path.resolve('..', 'global')

  try {
    await linkFromGlobal(['does-not-exist'], process.cwd(), await testDefaults({ globalPrefix }))
    t.fail('should have failed')
  } catch (err) {
    t.notOk(await exists(path.join(globalPrefix, 'node_modules', 'does-not-exist')))
  }
})

test('node_modules is pruned after linking', async (t: tape.Test) => {
  const project = prepare(t)

  await writeJsonFile('../is-positive/package.json', { name: 'is-positive', version: '1.0.0' })

  await installPkgs(['is-positive@1.0.0'], await testDefaults())

  t.ok(await exists('node_modules/.localhost+4873/is-positive/1.0.0/node_modules/is-positive/package.json'))

  await link(['../is-positive'], path.resolve('node_modules'), await testDefaults())

  t.notOk(await exists('node_modules/.localhost+4873/is-positive/1.0.0/node_modules/is-positive/package.json'), 'pruned')
})

test('relative link uses realpath when contained in a symlinked dir', async (t: tape.Test) => {
  const project = prepare(t)

  // `process.cwd()` is now `.tmp/X/project`.

  await ncp(pathToLocalPkg('symlink-workspace'), path.resolve('../symlink-workspace'))

  const app1RelPath = '../symlink-workspace/app1'
  const app2RelPath = '../symlink-workspace/app2'

  const app1 = path.resolve(app1RelPath)
  const app2 = path.resolve(app2RelPath)

  const dest = path.join(app2, 'packages/public')
  const src = path.resolve(app1, 'packages/public')

  t.comment(`${dest}->${src}`)

  // We must manually create the symlink so it works in Windows too.
  await symlink(src, dest)

  process.chdir(path.join(app2, `/packages/public/foo`))

  // `process.cwd()` is now `.tmp/X/symlink-workspace/app2/packages/public/foo`.

  const linkFrom = path.join(app1, `/packages/public/bar`)
  const linkTo = path.join(app2, `/packages/public/foo`, 'node_modules')

  await link([linkFrom], linkTo, await testDefaults())

  const linkToRelLink = await fs.readlink(path.join(linkTo, 'bar'))

  if (process.platform === 'win32') {
    t.equal(path.relative(linkToRelLink, path.join(src, 'bar')), '', 'link points to real location')
  } else {
    t.equal(linkToRelLink, '../../bar')

    // If we don't use real paths we get a link like this.
    t.notEqual(linkToRelLink, '../../../../../app1/packages/public/bar')
  }
})

test('relative link when an external shrinkwrap is used', async (t) => {
  const projects = prepare(t, [
    {
      name: 'project',
      version: '1.0.0',

      dependencies: {},
    },
  ])

  const opts = await testDefaults({ shrinkwrapDirectory: path.join('..') })
  await link([process.cwd()], path.resolve(process.cwd(), 'node_modules'), opts)

  const shr = await loadYamlFile(path.resolve('..', 'shrinkwrap.yaml'))

  t.deepEqual(shr && shr['importers'], {
    project: {
      dependencies: {
        project: 'link:',
      },
      specifiers: {},
    },
  })
})
