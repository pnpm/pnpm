import * as path from 'path'
import {
  addDependenciesToPackage,
  install,
  mutateModules,
} from 'supi'
import { prepareEmpty } from '@pnpm/prepare'
import { LifecycleLog } from '@pnpm/core-loggers'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import promisifyTape from 'tape-promise'
import { testDefaults } from '../utils'
import rimraf = require('@zkochan/rimraf')
import loadJsonFile = require('load-json-file')
import fs = require('mz/fs')
import exists = require('path-exists')
import PATH = require('path-name')
import sinon = require('sinon')
import tape = require('tape')

const test = promisifyTape(tape)

test('run pre/postinstall scripts', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  const manifest = await addDependenciesToPackage({},
    ['pre-and-postinstall-scripts-example'],
    await testDefaults({ fastUnpack: false, targetDependenciesField: 'devDependencies' })
  )

  {
    t.notOk(await exists('node_modules/pre-and-postinstall-scripts-example/generated-by-prepare.js'))
    t.ok(await exists('node_modules/pre-and-postinstall-scripts-example/generated-by-preinstall.js'))

    const generatedByPreinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-preinstall')
    t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is available')

    const generatedByPostinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-postinstall')
    t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is available')
  }

  await rimraf('node_modules')

  // testing that the packages are not installed even though they are in lockfile
  // and that their scripts are not tried to be executed

  await install(manifest, await testDefaults({ fastUnpack: false, production: true }))

  {
    const generatedByPreinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-preinstall')
    t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is not available')

    const generatedByPostinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-postinstall')
    t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is not available')
  }

  const lockfile = await project.readLockfile()
  t.ok(lockfile.packages['/pre-and-postinstall-scripts-example/1.0.0'].requiresBuild, 'requiresBuild: true added to lockfile')
})

test('run pre/postinstall scripts, when PnP is used and no symlinks', async (t: tape.Test) => {
  prepareEmpty(t)
  await addDependenciesToPackage({},
    ['pre-and-postinstall-scripts-example'],
    await testDefaults({
      fastUnpack: false,
      enablePnp: true,
      symlink: false,
      targetDependenciesField: 'devDependencies',
    })
  )

  const pkgDir = 'node_modules/.pnpm/pre-and-postinstall-scripts-example@1.0.0/node_modules/pre-and-postinstall-scripts-example'
  t.notOk(await exists(path.resolve(pkgDir, 'generated-by-prepare.js')))
  t.ok(await exists(path.resolve(pkgDir, 'generated-by-preinstall.js')))
  t.ok(await exists(path.resolve(pkgDir, 'generated-by-postinstall.js')))
})

test('testing that the bins are linked when the package with the bins was already in node_modules', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  const manifest = await addDependenciesToPackage({}, ['hello-world-js-bin'], await testDefaults({ fastUnpack: false }))
  await addDependenciesToPackage(manifest, ['pre-and-postinstall-scripts-example'], await testDefaults({ fastUnpack: false, targetDependenciesField: 'devDependencies' }))

  const generatedByPreinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-preinstall')
  t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is available')

  const generatedByPostinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-postinstall')
  t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is available')
})

test('run install scripts', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  await addDependenciesToPackage({}, ['install-script-example'], await testDefaults({ fastUnpack: false }))

  const generatedByInstall = project.requireModule('install-script-example/generated-by-install')
  t.ok(typeof generatedByInstall === 'function', 'generatedByInstall() is available')
})

test('run install scripts in the current project', async (t: tape.Test) => {
  prepareEmpty(t)
  const manifest = await addDependenciesToPackage({
    scripts: {
      install: 'node -e "process.stdout.write(\'install\')" | json-append output.json',
      postinstall: 'node -e "process.stdout.write(\'postinstall\')" | json-append output.json',
      preinstall: 'node -e "process.stdout.write(\'preinstall\')" | json-append output.json',
    },
  }, ['json-append@1.1.1'], await testDefaults({ fastUnpack: false }))
  await install(manifest, await testDefaults({ fastUnpack: false }))

  const output = await loadJsonFile<string[]>('output.json')

  t.deepEqual(output, ['preinstall', 'install', 'postinstall'])
})

test('run install scripts in the current project when its name is different than its directory', async (t: tape.Test) => {
  prepareEmpty(t)
  const manifest = await addDependenciesToPackage({
    name: 'different-name',
    scripts: {
      install: 'node -e "process.stdout.write(\'install\')" | json-append output.json',
      postinstall: 'node -e "process.stdout.write(\'postinstall\')" | json-append output.json',
      preinstall: 'node -e "process.stdout.write(\'preinstall\')" | json-append output.json',
    },
  }, ['json-append@1.1.1'], await testDefaults({ fastUnpack: false }))
  await install(manifest, await testDefaults({ fastUnpack: false }))

  const output = await loadJsonFile('output.json')

  t.deepEqual(output, ['preinstall', 'install', 'postinstall'])
})

test('do not run install scripts if unsafePerm is false', async (t: tape.Test) => {
  prepareEmpty(t)
  const opts = await testDefaults({ fastUnpack: false, unsafePerm: false })
  const manifest = await addDependenciesToPackage({
    name: 'different-name',
    scripts: {
      install: 'node -e "process.stdout.write(\'install\')" | json-append output.json',
      postinstall: 'node -e "process.stdout.write(\'postinstall\')" | json-append output.json',
      preinstall: 'node -e "process.stdout.write(\'preinstall\')" | json-append output.json',
    },
  }, ['json-append@1.1.1'], opts)
  await install(manifest, opts)

  const outputExists = await exists('output.json')

  t.false(outputExists, 'no output expected as install scripts should not run')
})

test('installation fails if lifecycle script fails', async (t: tape.Test) => {
  prepareEmpty(t)

  try {
    await install({
      scripts: {
        preinstall: 'exit 1',
      },
    }, await testDefaults({ fastUnpack: false }))
    t.fail('should have failed')
  } catch (err) {
    t.equal(err.code, 'ELIFECYCLE', 'failed with correct error code')
  }
})

test('INIT_CWD is always set to lockfile directory', async (t: tape.Test) => {
  prepareEmpty(t)
  const rootDir = process.cwd()
  await fs.mkdir('subd')
  process.chdir('subd')
  await mutateModules([
    {
      buildIndex: 0,
      mutation: 'install',
      manifest: {
        dependencies: {
          'json-append': '1.1.1',
          'write-lifecycle-env': '1.0.0',
        },
        scripts: {
          install: 'node -e "process.stdout.write(process.env.INIT_CWD)" | json-append output.json',
        },
      },
      rootDir,
    },
  ], await testDefaults({
    fastUnpack: false,
    lockfileDir: rootDir,
  }))

  const childEnv = await loadJsonFile<{ INIT_CWD: string }>(path.join(rootDir, 'node_modules/write-lifecycle-env/env.json'))
  t.equal(childEnv.INIT_CWD, rootDir)

  const output = await loadJsonFile(path.join(rootDir, 'output.json'))
  t.deepEqual(output, [process.cwd()])
})

// TODO: duplicate this test to @pnpm/lifecycle
test("reports child's output", async (t: tape.Test) => {
  prepareEmpty(t)

  const reporter = sinon.spy()

  await addDependenciesToPackage({}, ['count-to-10'], await testDefaults({ fastUnpack: false, reporter }))

  t.ok(reporter.calledWithMatch({
    depPath: '/count-to-10/1.0.0',
    level: 'debug',
    name: 'pnpm:lifecycle',
    script: 'node postinstall',
    stage: 'postinstall',
  } as LifecycleLog))
  t.ok(reporter.calledWithMatch({
    depPath: '/count-to-10/1.0.0',
    level: 'debug',
    line: '1',
    name: 'pnpm:lifecycle',
    stage: 'postinstall',
    stdio: 'stdout',
  } as LifecycleLog))
  t.ok(reporter.calledWithMatch({
    depPath: '/count-to-10/1.0.0',
    level: 'debug',
    line: '2',
    name: 'pnpm:lifecycle',
    stage: 'postinstall',
    stdio: 'stdout',
  } as LifecycleLog))
  t.ok(reporter.calledWithMatch({
    depPath: '/count-to-10/1.0.0',
    level: 'debug',
    line: '6',
    name: 'pnpm:lifecycle',
    stage: 'postinstall',
    stdio: 'stderr',
  } as LifecycleLog))
  t.ok(reporter.calledWithMatch({
    depPath: '/count-to-10/1.0.0',
    exitCode: 0,
    level: 'debug',
    name: 'pnpm:lifecycle',
    stage: 'postinstall',
  } as LifecycleLog))
})

test("reports child's close event", async (t: tape.Test) => {
  prepareEmpty(t)

  const reporter = sinon.spy()

  try {
    await addDependenciesToPackage({}, ['failing-postinstall'], await testDefaults({ reporter }))
    t.fail()
  } catch (err) {
    t.ok(reporter.calledWithMatch({
      depPath: '/failing-postinstall/1.0.0',
      exitCode: 1,
      level: 'debug',
      name: 'pnpm:lifecycle',
      stage: 'postinstall',
    } as LifecycleLog))
  }
})

test('lifecycle scripts have access to node-gyp', async (t: tape.Test) => {
  prepareEmpty(t)

  // `npm test` adds node-gyp to the PATH
  // it is removed here to test that pnpm adds it
  const initialPath = process.env.PATH

  if (typeof initialPath !== 'string') throw new Error('PATH is not defined')

  process.env[PATH] = initialPath
    .split(path.delimiter)
    .filter((p: string) => !p.includes('node-gyp-bin') && !p.includes('npm'))
    .join(path.delimiter)

  await addDependenciesToPackage({}, ['drivelist@5.1.8'], await testDefaults({ fastUnpack: false }))

  process.env[PATH] = initialPath

  t.pass("drivelist's install script has found node-gyp in PATH")
})

test('run lifecycle scripts of dependent packages after running scripts of their deps', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await addDependenciesToPackage({}, ['with-postinstall-a'], await testDefaults({ fastUnpack: false }))

  t.ok(+project.requireModule('.pnpm/with-postinstall-b@1.0.0/node_modules/with-postinstall-b/output.json')[0] < +project.requireModule('with-postinstall-a/output.json')[0])
})

test('run prepare script for git-hosted dependencies', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await addDependenciesToPackage({}, ['zkochan/install-scripts-example#prepare'], await testDefaults({ fastUnpack: false }))

  const scripts = project.requireModule('install-scripts-example-for-pnpm/output.json')
  t.equal(scripts[0], 'preinstall')
  t.equal(scripts[1], 'install')
  t.equal(scripts[2], 'postinstall')
  t.equal(scripts[3], 'prepare')

  const lockfile = await project.readLockfile()
  t.ok(lockfile.packages['github.com/zkochan/install-scripts-example/2de638b8b572cd1e87b74f4540754145fb2c0ebb'].prepare === true, `prepare field added to ${WANTED_LOCKFILE}`)
})

test('lifecycle scripts run before linking bins', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  const manifest = await addDependenciesToPackage({}, ['generated-bins'], await testDefaults({ fastUnpack: false }))

  await project.isExecutable('.bin/cmd1')
  await project.isExecutable('.bin/cmd2')

  await rimraf('node_modules')

  await mutateModules(
    [
      {
        buildIndex: 0,
        manifest,
        mutation: 'install',
        rootDir: process.cwd(),
      },
    ],
    await testDefaults({ frozenLockfile: true })
  )

  await project.isExecutable('.bin/cmd1')
  await project.isExecutable('.bin/cmd2')
})

test('hoisting does not fail on commands that will be created by lifecycle scripts on a later stage', async (t: tape.Test) => {
  prepareEmpty(t)

  const manifest = await addDependenciesToPackage({}, ['has-generated-bins-as-dep'], await testDefaults({ fastUnpack: false, hoistPattern: '*' }))

  // await project.isExecutable('.pnpm/node_modules/.bin/cmd1')
  // await project.isExecutable('.pnpm/node_modules/.bin/cmd2')

  // Testing the same with headless installation
  await rimraf('node_modules')

  await mutateModules(
    [
      {
        buildIndex: 0,
        manifest,
        mutation: 'install',
        rootDir: process.cwd(),
      },
    ],
    await testDefaults({ frozenLockfile: true, hoistPattern: '*' })
  )

  // await project.isExecutable('.pnpm/node_modules/.bin/cmd1')
  // await project.isExecutable('.pnpm/node_modules/.bin/cmd2')
  t.pass('installation did not fail')
})

test('bins are linked even if lifecycle scripts are ignored', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  const manifest = await addDependenciesToPackage(
    {},
    [
      'pkg-with-peer-having-bin',
      'peer-with-bin',
      'pre-and-postinstall-scripts-example',
    ],
    await testDefaults({ fastUnpack: false, ignoreScripts: true })
  )

  await project.isExecutable('.bin/peer-with-bin')
  await project.isExecutable('pkg-with-peer-having-bin/node_modules/.bin/hello-world-js-bin')

  // Verifying that the scripts were ignored
  t.ok(await exists('node_modules/pre-and-postinstall-scripts-example/package.json'))
  t.notOk(await exists('node_modules/pre-and-postinstall-scripts-example/generated-by-preinstall.js'), 'scripts were ignored indeed')

  await rimraf('node_modules')

  await mutateModules(
    [
      {
        buildIndex: 0,
        manifest,
        mutation: 'install',
        rootDir: process.cwd(),
      },
    ],
    await testDefaults({ frozenLockfile: true, ignoreScripts: true })
  )

  await project.isExecutable('.bin/peer-with-bin')
  await project.isExecutable('pkg-with-peer-having-bin/node_modules/.bin/hello-world-js-bin')

  // Verifying that the scripts were ignored
  t.ok(await exists('node_modules/pre-and-postinstall-scripts-example/package.json'))
  t.notOk(await exists('node_modules/pre-and-postinstall-scripts-example/generated-by-preinstall.js'), 'scripts were ignored indeed')
})

test('dependency should not be added to current lockfile if it was not built successfully during headless install', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  const manifest = await addDependenciesToPackage(
    {},
    [
      'package-that-cannot-be-installed@0.0.0',
    ],
    await testDefaults({
      ignoreScripts: true,
      lockfileOnly: true,
    })
  )

  let err
  try {
    await mutateModules(
      [
        {
          buildIndex: 0,
          manifest,
          mutation: 'install',
          rootDir: process.cwd(),
        },
      ],
      await testDefaults({ frozenLockfile: true })
    )
  } catch (_err) {
    err = _err
  }

  t.ok(err)

  t.notOk(await project.readCurrentLockfile())
})

test('scripts have access to unlisted bins when hoisting is used', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await addDependenciesToPackage(
    {},
    ['pkg-that-calls-unlisted-dep-in-hooks'],
    await testDefaults({ fastUnpack: false, hoistPattern: '*' })
  )

  t.deepEqual(project.requireModule('pkg-that-calls-unlisted-dep-in-hooks/output.json'), ['Hello world!'])
})
