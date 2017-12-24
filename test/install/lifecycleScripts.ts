import tape = require('tape')
import promisifyTape from 'tape-promise'
import sinon = require('sinon')
import {
  installPkgs,
  install,
  LifecycleLog,
} from 'supi'
import {
  prepare,
  testDefaults,
} from '../utils'
import path = require('path')
import loadJsonFile = require('load-json-file')
import rimraf = require('rimraf-then')
import exists = require('path-exists')
import PATH = require('path-name')

const pkgRoot = path.join(__dirname, '..', '..')
const pnpmPkg = loadJsonFile.sync(path.join(pkgRoot, 'package.json'))

const test = promisifyTape(tape)

test('run pre/postinstall scripts', async function (t: tape.Test) {
  const project = prepare(t)
  await installPkgs(['pre-and-postinstall-scripts-example'], testDefaults({saveDev: true}))

  const generatedByPreinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-preinstall')
  t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is available')

  const generatedByPostinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-postinstall')
  t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is available')

  await rimraf('node_modules')

  // testing that the packages are not installed even though they are in shrinkwrap
  // and that their scripts are not tried to be executed

  await install(testDefaults({production: true}))

  {
    const generatedByPreinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-preinstall')
    t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is not available')

    const generatedByPostinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-postinstall')
    t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is not available')
  }
})

test('testing that the bins are linked when the package with the bins was already in node_modules', async function (t: tape.Test) {
  const project = prepare(t)

  await installPkgs(['hello-world-js-bin'], testDefaults())
  await installPkgs(['pre-and-postinstall-scripts-example'], testDefaults({saveDev: true}))

  const generatedByPreinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-preinstall')
  t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is available')

  const generatedByPostinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-postinstall')
  t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is available')
})

test('run install scripts', async function (t) {
  const project = prepare(t)
  await installPkgs(['install-script-example'], testDefaults())

  const generatedByInstall = project.requireModule('install-script-example/generated-by-install')
  t.ok(typeof generatedByInstall === 'function', 'generatedByInstall() is available')
})

test('run install scripts in the current project', async (t: tape.Test) => {
  const project = prepare(t, {
    scripts: {
      preinstall: `node -e "process.stdout.write('preinstall')" | json-append output.json`,
      install: `node -e "process.stdout.write('install')" | json-append output.json`,
      postinstall: `node -e "process.stdout.write('postinstall')" | json-append output.json`,
    }
  })
  await installPkgs(['json-append@1.1.1'], testDefaults())
  await install(testDefaults())

  const output = await loadJsonFile('output.json')

  t.deepEqual(output, ['preinstall', 'install', 'postinstall'])
})

test('run install scripts in the current project when its name is different than its directory', async (t: tape.Test) => {
  const project = prepare(t, {
    name: 'different-name',
    scripts: {
      preinstall: `node -e "process.stdout.write('preinstall')" | json-append output.json`,
      install: `node -e "process.stdout.write('install')" | json-append output.json`,
      postinstall: `node -e "process.stdout.write('postinstall')" | json-append output.json`,
    }
  })
  await installPkgs(['json-append@1.1.1'], testDefaults())
  await install(testDefaults())

  const output = await loadJsonFile('output.json')

  t.deepEqual(output, ['preinstall', 'install', 'postinstall'])
})

test('do not run install scripts if unsafePerm is false', async (t: tape.Test) => {
  const project = prepare(t, {
    name: 'different-name',
    scripts: {
      preinstall: `node -e "process.stdout.write('preinstall')" | json-append output.json`,
      install: `node -e "process.stdout.write('install')" | json-append output.json`,
      postinstall: `node -e "process.stdout.write('postinstall')" | json-append output.json`,
    }
  })
  const opts = Object.assign(testDefaults(), { unsafePerm: false })
  await installPkgs(['json-append@1.1.1'], opts)
  await install(opts)

  let outputExists = await exists('output.json')

  t.false(outputExists, 'no output expected as install scripts should not run')
})

test('installation fails if lifecycle script fails', async (t: tape.Test) => {
  const project = prepare(t, {
    scripts: {
      preinstall: 'exit 1'
    },
  })

  try {
    await install(testDefaults())
    t.fail('should have failed')
  } catch (err) {
    t.equal(err['code'], 'ELIFECYCLE', 'failed with correct error code')
  }
})

// TODO: unskip
// For some reason this fails on CI environments
test['skip']('creates env for scripts', async (t: tape.Test) => {
  const project = prepare(t, {
    scripts: {
      install: `node -e "process.stdout.write(process.env.INIT_CWD)" | json-append output.json`,
    }
  })
  await installPkgs(['json-append@1.1.1'], testDefaults())
  await install(testDefaults())

  const output = await loadJsonFile('output.json')

  t.deepEqual(output, [process.cwd()])
})

test('INIT_CWD is set correctly', async (t: tape.Test) => {
  const project = prepare(t)
  await installPkgs(['write-lifecycle-env'], testDefaults())

  const childEnv = await loadJsonFile(path.resolve('node_modules', 'write-lifecycle-env', 'env.json'))

  t.equal(childEnv['INIT_CWD'], process.cwd())
})

test("reports child's output", async (t: tape.Test) => {
  const project = prepare(t)

  const reporter = sinon.spy()

  await installPkgs(['count-to-10'], testDefaults({reporter}))

  t.ok(reporter.calledWithMatch(<LifecycleLog>{
    name: 'pnpm:lifecycle',
    level: 'info',
    line: '1',
    pkgId: 'localhost+4873/count-to-10/1.0.0',
  }))
  t.ok(reporter.calledWithMatch(<LifecycleLog>{
    name: 'pnpm:lifecycle',
    level: 'info',
    line: '2',
    pkgId: 'localhost+4873/count-to-10/1.0.0',
  }))
  t.ok(reporter.calledWithMatch(<LifecycleLog>{
    name: 'pnpm:lifecycle',
    level: 'error',
    line: '6',
    pkgId: 'localhost+4873/count-to-10/1.0.0',
  }))
  t.ok(reporter.calledWithMatch(<LifecycleLog>{
    name: 'pnpm:lifecycle',
    exitCode: 0,
    level: 'info',
    script: 'postinstall',
    pkgId: 'localhost+4873/count-to-10/1.0.0',
  }))
})

test("reports child's close event", async (t: tape.Test) => {
  const project = prepare(t)

  const reporter = sinon.spy()

  try {
    await installPkgs(['failing-postinstall'], testDefaults({reporter}))
    t.fail()
  } catch (err) {
    t.ok(reporter.calledWithMatch(<LifecycleLog>{
      name: 'pnpm:lifecycle',
      exitCode: 1,
      level: 'error',
      script: 'postinstall',
      pkgId: 'localhost+4873/failing-postinstall/1.0.0',
    }))
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

  await installPkgs(['drivelist@5.1.8'], testDefaults())

  process.env[PATH] = initialPath

  t.pass("drivelist's install script has found node-gyp in PATH")
})
