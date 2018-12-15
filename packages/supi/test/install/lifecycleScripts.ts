import { LifecycleLog } from '@pnpm/core-loggers'
import prepare from '@pnpm/prepare'
import loadJsonFile from 'load-json-file'
import path = require('path')
import exists = require('path-exists')
import PATH = require('path-name')
import rimraf = require('rimraf-then')
import sinon = require('sinon')
import {
  addDependenciesToPackage,
  install,
} from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { testDefaults } from '../utils'

const pkgRoot = path.join(__dirname, '..', '..')

const test = promisifyTape(tape)

test('run pre/postinstall scripts', async (t: tape.Test) => {
  const project = prepare(t)
  await addDependenciesToPackage(['pre-and-postinstall-scripts-example'], await testDefaults({ targetDependenciesField: 'devDependencies' }))

  {
    t.notOk(await exists('node_modules/pre-and-postinstall-scripts-example/generated-by-prepare.js'))
    t.ok(await exists('node_modules/pre-and-postinstall-scripts-example/generated-by-preinstall.js'))

    const generatedByPreinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-preinstall')
    t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is available')

    const generatedByPostinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-postinstall')
    t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is available')
  }

  await rimraf('node_modules')

  // testing that the packages are not installed even though they are in shrinkwrap
  // and that their scripts are not tried to be executed

  await install(await testDefaults({ production: true }))

  {
    const generatedByPreinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-preinstall')
    t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is not available')

    const generatedByPostinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-postinstall')
    t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is not available')
  }

  const shr = await project.loadShrinkwrap()
  t.ok(shr.packages['/pre-and-postinstall-scripts-example/1.0.0'].requiresBuild, 'requiresBuild: true added to shrinkwrap')
})

test('testing that the bins are linked when the package with the bins was already in node_modules', async (t: tape.Test) => {
  const project = prepare(t)

  await addDependenciesToPackage(['hello-world-js-bin'], await testDefaults())
  await addDependenciesToPackage(['pre-and-postinstall-scripts-example'], await testDefaults({ targetDependenciesField: 'devDependencies' }))

  const generatedByPreinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-preinstall')
  t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is available')

  const generatedByPostinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-postinstall')
  t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is available')
})

test('run install scripts', async (t: tape.Test) => {
  const project = prepare(t)
  await addDependenciesToPackage(['install-script-example'], await testDefaults())

  const generatedByInstall = project.requireModule('install-script-example/generated-by-install')
  t.ok(typeof generatedByInstall === 'function', 'generatedByInstall() is available')
})

test('run install scripts in the current project', async (t: tape.Test) => {
  const project = prepare(t, {
    scripts: {
      install: `node -e "process.stdout.write('install')" | json-append output.json`,
      postinstall: `node -e "process.stdout.write('postinstall')" | json-append output.json`,
      preinstall: `node -e "process.stdout.write('preinstall')" | json-append output.json`,
    },
  })
  await addDependenciesToPackage(['json-append@1.1.1'], await testDefaults())
  await install(await testDefaults())

  const output = await loadJsonFile<string[]>('output.json')

  t.deepEqual(output, ['preinstall', 'install', 'postinstall'])
})

test('run install scripts in the current project when its name is different than its directory', async (t: tape.Test) => {
  const project = prepare(t, {
    name: 'different-name',
    scripts: {
      install: `node -e "process.stdout.write('install')" | json-append output.json`,
      postinstall: `node -e "process.stdout.write('postinstall')" | json-append output.json`,
      preinstall: `node -e "process.stdout.write('preinstall')" | json-append output.json`,
    },
  })
  await addDependenciesToPackage(['json-append@1.1.1'], await testDefaults())
  await install(await testDefaults())

  const output = await loadJsonFile('output.json')

  t.deepEqual(output, ['preinstall', 'install', 'postinstall'])
})

test('do not run install scripts if unsafePerm is false', async (t: tape.Test) => {
  const project = prepare(t, {
    name: 'different-name',
    scripts: {
      install: `node -e "process.stdout.write('install')" | json-append output.json`,
      postinstall: `node -e "process.stdout.write('postinstall')" | json-append output.json`,
      preinstall: `node -e "process.stdout.write('preinstall')" | json-append output.json`,
    },
  })
  const opts = await testDefaults({ unsafePerm: false })
  await addDependenciesToPackage(['json-append@1.1.1'], opts)
  await install(opts)

  const outputExists = await exists('output.json')

  t.false(outputExists, 'no output expected as install scripts should not run')
})

test('installation fails if lifecycle script fails', async (t: tape.Test) => {
  const project = prepare(t, {
    scripts: {
      preinstall: 'exit 1',
    },
  })

  try {
    await install(await testDefaults())
    t.fail('should have failed')
  } catch (err) {
    t.equal(err.code, 'ELIFECYCLE', 'failed with correct error code')
  }
})

// TODO: unskip
// For some reason this fails on CI environments
// tslint:disable-next-line:no-string-literal
test['skip']('creates env for scripts', async (t: tape.Test) => {
  const project = prepare(t, {
    scripts: {
      install: `node -e "process.stdout.write(process.env.INIT_CWD)" | json-append output.json`,
    },
  })
  await addDependenciesToPackage(['json-append@1.1.1'], await testDefaults())
  await install(await testDefaults())

  const output = await loadJsonFile('output.json')

  t.deepEqual(output, [process.cwd()])
})

test('INIT_CWD is set correctly', async (t: tape.Test) => {
  const project = prepare(t)
  await addDependenciesToPackage(['write-lifecycle-env'], await testDefaults())

  const childEnv = await loadJsonFile<{ INIT_CWD: string }>(path.resolve('node_modules', 'write-lifecycle-env', 'env.json'))

  t.equal(childEnv.INIT_CWD, process.cwd())
})

// TODO: duplicate this test to @pnpm/lifecycle
test("reports child's output", async (t: tape.Test) => {
  const project = prepare(t)

  const reporter = sinon.spy()

  await addDependenciesToPackage(['count-to-10'], await testDefaults({ reporter }))

  t.ok(reporter.calledWithMatch({
    depPath: 'localhost+4873/count-to-10/1.0.0',
    level: 'debug',
    name: 'pnpm:lifecycle',
    script: 'node postinstall',
    stage: 'postinstall',
  } as LifecycleLog))
  t.ok(reporter.calledWithMatch({
    depPath: 'localhost+4873/count-to-10/1.0.0',
    level: 'debug',
    line: '1',
    name: 'pnpm:lifecycle',
    stage: 'postinstall',
    stdio: 'stdout',
  } as LifecycleLog))
  t.ok(reporter.calledWithMatch({
    depPath: 'localhost+4873/count-to-10/1.0.0',
    level: 'debug',
    line: '2',
    name: 'pnpm:lifecycle',
    stage: 'postinstall',
    stdio: 'stdout',
  } as LifecycleLog))
  t.ok(reporter.calledWithMatch({
    depPath: 'localhost+4873/count-to-10/1.0.0',
    level: 'debug',
    line: '6',
    name: 'pnpm:lifecycle',
    stage: 'postinstall',
    stdio: 'stderr',
  } as LifecycleLog))
  t.ok(reporter.calledWithMatch({
    depPath: 'localhost+4873/count-to-10/1.0.0',
    exitCode: 0,
    level: 'debug',
    name: 'pnpm:lifecycle',
    stage: 'postinstall',
  } as LifecycleLog))
})

test("reports child's close event", async (t: tape.Test) => {
  const project = prepare(t)

  const reporter = sinon.spy()

  try {
    await addDependenciesToPackage(['failing-postinstall'], await testDefaults({ reporter }))
    t.fail()
  } catch (err) {
    t.ok(reporter.calledWithMatch({
      depPath: 'localhost+4873/failing-postinstall/1.0.0',
      exitCode: 1,
      level: 'debug',
      name: 'pnpm:lifecycle',
      stage: 'postinstall',
    } as LifecycleLog))
  }
})

test('lifecycle scripts have access to node-gyp', async (t: tape.Test) => {
  const project = prepare(t)

  // `npm test` adds node-gyp to the PATH
  // it is removed here to test that pnpm adds it
  const initialPath = process.env.PATH

  if (typeof initialPath !== 'string') throw new Error('PATH is not defined')

  process.env[PATH] = initialPath
    .split(path.delimiter)
    .filter((p: string) => !p.includes('node-gyp-bin') && !p.includes('npm'))
    .join(path.delimiter)

  await addDependenciesToPackage(['drivelist@5.1.8'], await testDefaults())

  process.env[PATH] = initialPath

  t.pass("drivelist's install script has found node-gyp in PATH")
})

test('run lifecycle scripts of dependent packages after running scripts of their deps', async (t: tape.Test) => {
  const project = prepare(t)

  await addDependenciesToPackage(['with-postinstall-a'], await testDefaults())

  t.ok(+project.requireModule('.localhost+4873/with-postinstall-b/1.0.0/node_modules/with-postinstall-b/output.json')[0] < +project.requireModule('with-postinstall-a/output.json')[0])
})

test('run prepare script for git-hosted dependencies', async (t: tape.Test) => {
  const project = prepare(t)

  await addDependenciesToPackage(['zkochan/install-scripts-example#prepare'], await testDefaults())

  const scripts = project.requireModule('install-scripts-example-for-pnpm/output.json')
  t.equal(scripts[0], 'preinstall')
  t.equal(scripts[1], 'install')
  t.equal(scripts[2], 'postinstall')
  t.equal(scripts[3], 'prepare')

  const shr = await project.loadShrinkwrap()
  t.ok(shr.packages['github.com/zkochan/install-scripts-example/2de638b8b572cd1e87b74f4540754145fb2c0ebb'].prepare === true, 'prepare field added to shrinkwrap.yaml')
})
