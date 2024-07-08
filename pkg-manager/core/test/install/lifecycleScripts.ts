import * as path from 'path'
import fs from 'fs'
import { assertProject } from '@pnpm/assert-project'
import { type LifecycleLog } from '@pnpm/core-loggers'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import {
  addDependenciesToPackage,
  install,
  mutateModulesInSingleProject,
  type MutatedProject,
  mutateModules,
} from '@pnpm/core'
import { createTestIpcServer } from '@pnpm/test-ipc-server'
import { type ProjectRootDir } from '@pnpm/types'
import { restartWorkerPool } from '@pnpm/worker'
import { sync as rimraf } from '@zkochan/rimraf'
import isWindows from 'is-windows'
import loadJsonFile from 'load-json-file'
import PATH from 'path-name'
import sinon from 'sinon'
import { testDefaults } from '../utils'

const testOnNonWindows = isWindows() ? test.skip : test

test('run pre/postinstall scripts', async () => {
  const project = prepareEmpty()
  const manifest = await addDependenciesToPackage({},
    ['@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0'],
    testDefaults({ fastUnpack: false, targetDependenciesField: 'devDependencies' })
  )

  {
    expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-prepare.js')).toBeFalsy()
    expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeTruthy()

    const generatedByPreinstall = project.requireModule('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall')
    expect(typeof generatedByPreinstall).toBe('function')

    const generatedByPostinstall = project.requireModule('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall')
    expect(typeof generatedByPostinstall).toBe('function')
  }

  rimraf('node_modules')

  // testing that the packages are not installed even though they are in lockfile
  // and that their scripts are not tried to be executed

  await install(manifest, testDefaults({ fastUnpack: false, production: true }))

  {
    const generatedByPreinstall = project.requireModule('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall')
    expect(typeof generatedByPreinstall).toBe('function')

    const generatedByPostinstall = project.requireModule('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall')
    expect(typeof generatedByPostinstall).toBe('function')
  }
})

test('run pre/postinstall scripts, when PnP is used and no symlinks', async () => {
  prepareEmpty()
  await addDependenciesToPackage({},
    ['@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0'],
    testDefaults({
      fastUnpack: false,
      enablePnp: true,
      symlink: false,
      targetDependenciesField: 'devDependencies',
    })
  )

  const pkgDir = 'node_modules/.pnpm/@pnpm.e2e+pre-and-postinstall-scripts-example@1.0.0/node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example'
  expect(fs.existsSync(path.resolve(pkgDir, 'generated-by-prepare.js'))).toBeFalsy()
  expect(fs.existsSync(path.resolve(pkgDir, 'generated-by-preinstall.js'))).toBeTruthy()
  expect(fs.existsSync(path.resolve(pkgDir, 'generated-by-postinstall.js'))).toBeTruthy()
})

test('testing that the bins are linked when the package with the bins was already in node_modules', async () => {
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/hello-world-js-bin'], testDefaults({ fastUnpack: false }))
  await addDependenciesToPackage(manifest, ['@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0'], testDefaults({ fastUnpack: false, targetDependenciesField: 'devDependencies' }))

  const generatedByPreinstall = project.requireModule('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall')
  expect(typeof generatedByPreinstall).toBe('function')

  const generatedByPostinstall = project.requireModule('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall')
  expect(typeof generatedByPostinstall).toBe('function')
})

test('run install scripts', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['@pnpm.e2e/install-script-example'], testDefaults({ fastUnpack: false }))

  const generatedByInstall = project.requireModule('@pnpm.e2e/install-script-example/generated-by-install')
  expect(typeof generatedByInstall).toBe('function')
})

test('run install scripts in the current project', async () => {
  await using server = await createTestIpcServer()
  await using serverForDevPreinstall = await createTestIpcServer()
  prepareEmpty()
  const manifest = await addDependenciesToPackage({
    scripts: {
      'pnpm:devPreinstall': `node -e "console.log('pnpm:devPreinstall-' + process.cwd())" | ${serverForDevPreinstall.generateSendStdinScript()}`,
      install: `node -e "console.log('install-' + process.cwd())" | ${server.generateSendStdinScript()}`,
      postinstall: `node -e "console.log('postinstall-' + process.cwd())" | ${server.generateSendStdinScript()}`,
      preinstall: `node -e "console.log('preinstall-' + process.cwd())" | ${server.generateSendStdinScript()}`,
    },
  }, [], testDefaults({ fastUnpack: false }))
  await install(manifest, testDefaults({ fastUnpack: false }))

  expect(server.getLines()).toStrictEqual([`preinstall-${process.cwd()}`, `install-${process.cwd()}`, `postinstall-${process.cwd()}`])
  expect(serverForDevPreinstall.getLines()).toStrictEqual([
    // The pnpm:devPreinstall script runs twice in this test. Once for the
    // initial "addDependenciesToPackage" test setup stage and again for the
    // dedicated install afterwards.
    `pnpm:devPreinstall-${process.cwd()}`,
    `pnpm:devPreinstall-${process.cwd()}`,
  ])
})

test('run install scripts in the current project when its name is different than its directory', async () => {
  await using server = await createTestIpcServer()
  prepareEmpty()
  const manifest = await addDependenciesToPackage({
    name: 'different-name',
    scripts: {
      install: `node -e "console.log('install-' + process.cwd())" | ${server.generateSendStdinScript()}`,
      postinstall: `node -e "console.log('postinstall-' + process.cwd())" | ${server.generateSendStdinScript()}`,
      preinstall: `node -e "console.log('preinstall-' + process.cwd())" | ${server.generateSendStdinScript()}`,
    },
  }, [], testDefaults({ fastUnpack: false }))
  await install(manifest, testDefaults({ fastUnpack: false }))

  expect(server.getLines()).toStrictEqual([
    `preinstall-${process.cwd()}`,
    `install-${process.cwd()}`,
    `postinstall-${process.cwd()}`,
  ])
})

test('installation fails if lifecycle script fails', async () => {
  prepareEmpty()

  await expect(
    install({
      scripts: {
        preinstall: 'exit 1',
      },
    }, testDefaults({ fastUnpack: false }))
  ).rejects.toThrow(/@ preinstall: `exit 1`/)
})

test('INIT_CWD is always set to lockfile directory', async () => {
  prepareEmpty()
  const rootDir = process.cwd() as ProjectRootDir
  fs.mkdirSync('sub_dir')
  process.chdir('sub_dir')
  await mutateModulesInSingleProject({
    mutation: 'install',
    manifest: {
      dependencies: {
        '@pnpm.e2e/write-lifecycle-env': '1.0.0',
      },
      scripts: {
        install: 'node -e "fs.writeFileSync(\'output.json\', JSON.stringify(process.env.INIT_CWD))"',
      },
    },
    rootDir,
  }, testDefaults({
    fastUnpack: false,
    lockfileDir: rootDir,
  }))

  const childEnv = loadJsonFile.sync<{ INIT_CWD: string }>(path.join(rootDir, 'node_modules/@pnpm.e2e/write-lifecycle-env/env.json'))
  expect(childEnv.INIT_CWD).toBe(rootDir)

  const output = loadJsonFile.sync(path.join(rootDir, 'output.json'))
  expect(output).toStrictEqual(process.cwd())
})

// TODO: duplicate this test to @pnpm/lifecycle
test("reports child's output", async () => {
  prepareEmpty()

  const reporter = sinon.spy()

  await addDependenciesToPackage({}, ['@pnpm.e2e/count-to-10'], testDefaults({ fastUnpack: false, reporter }))

  expect(reporter.calledWithMatch({
    depPath: '@pnpm.e2e/count-to-10@1.0.0',
    level: 'debug',
    name: 'pnpm:lifecycle',
    script: 'node postinstall',
    stage: 'postinstall',
  } as LifecycleLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    depPath: '@pnpm.e2e/count-to-10@1.0.0',
    level: 'debug',
    line: '1',
    name: 'pnpm:lifecycle',
    stage: 'postinstall',
    stdio: 'stdout',
  } as LifecycleLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    depPath: '@pnpm.e2e/count-to-10@1.0.0',
    level: 'debug',
    line: '2',
    name: 'pnpm:lifecycle',
    stage: 'postinstall',
    stdio: 'stdout',
  } as LifecycleLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    depPath: '@pnpm.e2e/count-to-10@1.0.0',
    level: 'debug',
    line: '6',
    name: 'pnpm:lifecycle',
    stage: 'postinstall',
    stdio: 'stderr',
  } as LifecycleLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    depPath: '@pnpm.e2e/count-to-10@1.0.0',
    exitCode: 0,
    level: 'debug',
    name: 'pnpm:lifecycle',
    stage: 'postinstall',
  } as LifecycleLog)).toBeTruthy()
})

test("reports child's close event", async () => {
  prepareEmpty()

  const reporter = sinon.spy()

  await expect(
    addDependenciesToPackage({}, ['@pnpm.e2e/failing-postinstall'], testDefaults({ reporter }))
  ).rejects.toThrow()

  expect(reporter.calledWithMatch({
    depPath: '@pnpm.e2e/failing-postinstall@1.0.0',
    exitCode: 1,
    level: 'debug',
    name: 'pnpm:lifecycle',
    stage: 'postinstall',
  } as LifecycleLog)).toBeTruthy()
})

testOnNonWindows('lifecycle scripts have access to node-gyp', async () => {
  prepareEmpty()

  // `npm test` adds node-gyp to the PATH
  // it is removed here to test that pnpm adds it
  const initialPath = process.env[PATH]

  if (typeof initialPath !== 'string') throw new Error('PATH is not defined')

  process.env[PATH] = initialPath
    .split(path.delimiter)
    .filter((p: string) => !p.includes('node-gyp-bin') &&
      !p.includes(`${path.sep}npm${path.sep}`) &&
      !p.includes(`${path.sep}.npm${path.sep}`))
    .join(path.delimiter)

  await addDependenciesToPackage({}, ['drivelist@5.1.8'], testDefaults({ fastUnpack: false }))

  process.env[PATH] = initialPath
})

test('run lifecycle scripts of dependent packages after running scripts of their deps', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['@pnpm.e2e/with-postinstall-a'], testDefaults({ fastUnpack: false }))

  expect(+project.requireModule('.pnpm/@pnpm.e2e+with-postinstall-b@1.0.0/node_modules/@pnpm.e2e/with-postinstall-b/output.json')[0] < +project.requireModule('@pnpm.e2e/with-postinstall-a/output.json')[0]).toBeTruthy()
})

test('run prepare script for git-hosted dependencies', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['pnpm/test-git-fetch#8b333f12d5357f4f25a654c305c826294cb073bf'], testDefaults({ fastUnpack: false }))

  const scripts = project.requireModule('test-git-fetch/output.json')
  expect(scripts).toStrictEqual([
    'preinstall',
    'install',
    'postinstall',
    'prepare',
    'preinstall',
    'install',
    'postinstall',
  ])
})

test('lifecycle scripts run before linking bins', async () => {
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/generated-bins'], testDefaults({ fastUnpack: false }))

  project.isExecutable('.bin/cmd1')
  project.isExecutable('.bin/cmd2')

  rimraf('node_modules')

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ frozenLockfile: true }))

  project.isExecutable('.bin/cmd1')
  project.isExecutable('.bin/cmd2')
})

test('hoisting does not fail on commands that will be created by lifecycle scripts on a later stage', async () => {
  prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/has-generated-bins-as-dep'], testDefaults({ fastUnpack: false, hoistPattern: '*' }))

  // project.isExecutable('.pnpm/node_modules/.bin/cmd1')
  // project.isExecutable('.pnpm/node_modules/.bin/cmd2')

  // Testing the same with headless installation
  rimraf('node_modules')

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ frozenLockfile: true, hoistPattern: '*' }))

  // project.isExecutable('.pnpm/node_modules/.bin/cmd1')
  // project.isExecutable('.pnpm/node_modules/.bin/cmd2')
})

test('bins are linked even if lifecycle scripts are ignored', async () => {
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage(
    {},
    [
      '@pnpm.e2e/pkg-with-peer-having-bin',
      '@pnpm.e2e/peer-with-bin',
      '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0',
    ],
    testDefaults({ fastUnpack: false, ignoreScripts: true })
  )

  project.isExecutable('.bin/peer-with-bin')
  project.isExecutable('@pnpm.e2e/pkg-with-peer-having-bin/node_modules/.bin/hello-world-js-bin')

  // Verifying that the scripts were ignored
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/package.json')).toBeTruthy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeFalsy()

  rimraf('node_modules')

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ frozenLockfile: true, ignoreScripts: true }))

  project.isExecutable('.bin/peer-with-bin')
  project.isExecutable('@pnpm.e2e/pkg-with-peer-having-bin/node_modules/.bin/hello-world-js-bin')

  // Verifying that the scripts were ignored
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/package.json')).toBeTruthy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeFalsy()
})

test('dependency should not be added to current lockfile if it was not built successfully during headless install', async () => {
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage(
    {},
    [
      'package-that-cannot-be-installed@0.0.0', // TODO: this package should be replaced
    ],
    testDefaults({
      ignoreScripts: true,
      lockfileOnly: true,
    })
  )

  await expect(
    mutateModulesInSingleProject({
      manifest,
      mutation: 'install',
      rootDir: process.cwd() as ProjectRootDir,
    }, testDefaults({ frozenLockfile: true }))
  ).rejects.toThrow()

  expect(project.readCurrentLockfile()).toBeFalsy()
})

test('scripts have access to unlisted bins when hoisting is used', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage(
    {},
    ['@pnpm.e2e/pkg-that-calls-unlisted-dep-in-hooks'],
    testDefaults({ fastUnpack: false, hoistPattern: '*' })
  )

  expect(project.requireModule('@pnpm.e2e/pkg-that-calls-unlisted-dep-in-hooks/output.json')).toStrictEqual(['Hello world!'])
})

test('selectively ignore scripts in some dependencies by neverBuiltDependencies', async () => {
  prepareEmpty()
  const neverBuiltDependencies = ['@pnpm.e2e/pre-and-postinstall-scripts-example']
  const manifest = await addDependenciesToPackage({},
    ['@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0', '@pnpm.e2e/install-script-example'],
    testDefaults({ fastUnpack: false, neverBuiltDependencies })
  )

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeTruthy()

  rimraf('node_modules')

  await install(manifest, testDefaults({ fastUnpack: false, frozenLockfile: true, neverBuiltDependencies }))

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeTruthy()
})

test('throw an exception when both neverBuiltDependencies and onlyBuiltDependencies are used', async () => {
  prepareEmpty()

  await expect(
    addDependenciesToPackage(
      {},
      ['@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0'],
      testDefaults({ onlyBuiltDependencies: ['@pnpm.e2e/foo'], neverBuiltDependencies: ['@pnpm.e2e/bar'] })
    )
  ).rejects.toThrow(/Cannot have both/)
})

test('selectively allow scripts in some dependencies by onlyBuiltDependencies', async () => {
  prepareEmpty()
  const onlyBuiltDependencies = ['@pnpm.e2e/install-script-example']
  const manifest = await addDependenciesToPackage({},
    ['@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0', '@pnpm.e2e/install-script-example'],
    testDefaults({ fastUnpack: false, onlyBuiltDependencies })
  )

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeTruthy()

  rimraf('node_modules')

  await install(manifest, testDefaults({ fastUnpack: false, frozenLockfile: true, onlyBuiltDependencies }))

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeTruthy()
})

test('selectively allow scripts in some dependencies by onlyBuiltDependenciesFile', async () => {
  prepareEmpty()
  const onlyBuiltDependenciesFile = path.resolve('node_modules/@pnpm.e2e/build-allow-list/list.json')
  const manifest = await addDependenciesToPackage({},
    ['@pnpm.e2e/build-allow-list', '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0', '@pnpm.e2e/install-script-example'],
    testDefaults({ fastUnpack: false, onlyBuiltDependenciesFile })
  )

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeTruthy()

  rimraf('node_modules')

  await install(manifest, testDefaults({ fastUnpack: false, frozenLockfile: true, onlyBuiltDependenciesFile }))

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeTruthy()
})

test('selectively allow scripts in some dependencies by onlyBuiltDependenciesFile and onlyBuiltDependencies', async () => {
  prepareEmpty()
  const onlyBuiltDependenciesFile = path.resolve('node_modules/@pnpm.e2e/build-allow-list/list.json')
  const onlyBuiltDependencies = ['@pnpm.e2e/pre-and-postinstall-scripts-example']
  const manifest = await addDependenciesToPackage({},
    ['@pnpm.e2e/build-allow-list', '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0', '@pnpm.e2e/install-script-example'],
    testDefaults({ fastUnpack: false, onlyBuiltDependenciesFile, onlyBuiltDependencies })
  )

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeTruthy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeTruthy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeTruthy()

  rimraf('node_modules')

  await install(manifest, testDefaults({ fastUnpack: false, frozenLockfile: true, onlyBuiltDependenciesFile, onlyBuiltDependencies }))

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeTruthy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeTruthy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeTruthy()
})

test('lifecycle scripts have access to package\'s own binary by binary name', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({},
    ['@pnpm.e2e/runs-own-bin'],
    testDefaults({ fastUnpack: false })
  )

  project.isExecutable('.pnpm/@pnpm.e2e+runs-own-bin@1.0.0/node_modules/@pnpm.e2e/runs-own-bin/node_modules/.bin/runs-own-bin')
})

test('lifecycle scripts run after linking root dependencies', async () => {
  prepareEmpty()

  const manifest = {
    dependencies: {
      'is-positive': '1.0.0',
      '@pnpm.e2e/postinstall-requires-is-positive': '1.0.0',
    },
  }

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ fastUnpack: false }))

  rimraf('node_modules')

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ fastUnpack: false, frozenLockfile: true }))

  // if there was no exception, the test passed
})

test('ignore-dep-scripts', async () => {
  await using server1 = await createTestIpcServer()
  await using server2 = await createTestIpcServer()
  prepareEmpty()
  const manifest = {
    scripts: {
      'pnpm:devPreinstall': server2.sendLineScript('pnpm:devPreinstall'),
      install: server1.sendLineScript('install'),
      postinstall: server1.sendLineScript('postinstall'),
      preinstall: server1.sendLineScript('preinstall'),
    },
    dependencies: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': '1.0.0',
    },
  }
  await install(manifest, testDefaults({ fastUnpack: false, ignoreDepScripts: true }))

  expect(server1.getLines()).toStrictEqual(['preinstall', 'install', 'postinstall'])
  expect(server2.getLines()).toStrictEqual(['pnpm:devPreinstall'])

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeFalsy()

  rimraf('node_modules')
  server1.clear()
  server2.clear()
  await install(manifest, testDefaults({ fastUnpack: false, ignoreDepScripts: true }))

  expect(server1.getLines()).toStrictEqual(['preinstall', 'install', 'postinstall'])
  expect(server2.getLines()).toStrictEqual(['pnpm:devPreinstall'])

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeFalsy()
})

test('run pre/postinstall scripts in a workspace that uses node-linker=hoisted', async () => {
  await restartWorkerPool()
  const projects = preparePackages([
    {
      location: 'project-1',
      package: { name: 'project-1' },
    },
    {
      location: 'project-2',
      package: { name: 'project-2' },
    },
    {
      location: 'project-3',
      package: { name: 'project-3' },
    },
    {
      location: 'project-4',
      package: { name: 'project-4' },
    },
  ])
  const importers: MutatedProject[] = [
    {
      mutation: 'install',
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-3') as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-4') as ProjectRootDir,
    },
  ]
  const allProjects = [
    {
      buildIndex: 0,
      manifest: {
        name: 'project-1',
        version: '1.0.0',

        dependencies: {
          '@pnpm.e2e/pre-and-postinstall-scripts-example': '1',
        },
      },
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'project-2',
        version: '1.0.0',

        dependencies: {
          '@pnpm.e2e/pre-and-postinstall-scripts-example': '1',
        },
      },
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'project-3',
        version: '1.0.0',

        dependencies: {
          '@pnpm.e2e/pre-and-postinstall-scripts-example': '2',
        },
      },
      rootDir: path.resolve('project-3') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'project-4',
        version: '1.0.0',

        dependencies: {
          '@pnpm.e2e/pre-and-postinstall-scripts-example': '2',
        },
      },
      rootDir: path.resolve('project-4') as ProjectRootDir,
    },
  ]
  await mutateModules(importers, testDefaults({
    allProjects,
    fastUnpack: false,
    nodeLinker: 'hoisted',
  }))
  const rootProject = assertProject(process.cwd())
  rootProject.has('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  rootProject.has('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')
  projects['project-1'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  projects['project-1'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')
  projects['project-2'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  projects['project-2'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')
  projects['project-3'].has('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  projects['project-3'].has('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')
  projects['project-4'].has('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  projects['project-4'].has('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')
})

test('run pre/postinstall scripts in a project that uses node-linker=hoisted. Should not fail on repeat install', async () => {
  const project = prepareEmpty()
  const manifest = await addDependenciesToPackage({},
    ['@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0'],
    testDefaults({ fastUnpack: false, targetDependenciesField: 'devDependencies', nodeLinker: 'hoisted', sideEffectsCacheRead: true, sideEffectsCacheWrite: true })
  )

  {
    expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-prepare.js')).toBeFalsy()
    expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeTruthy()

    const generatedByPreinstall = project.requireModule('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall')
    expect(typeof generatedByPreinstall).toBe('function')

    const generatedByPostinstall = project.requireModule('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall')
    expect(typeof generatedByPostinstall).toBe('function')
  }

  const reporter = jest.fn()
  await addDependenciesToPackage(manifest,
    ['example@npm:@pnpm.e2e/pre-and-postinstall-scripts-example@2.0.0'],
    testDefaults({
      fastUnpack: false,
      targetDependenciesField: 'devDependencies',
      nodeLinker: 'hoisted',
      reporter,
      sideEffectsCacheRead: true,
      sideEffectsCacheWrite: true,
    })
  )

  expect(reporter).not.toHaveBeenCalledWith(expect.objectContaining({
    level: 'warn',
    message: `An error occurred while uploading ${path.resolve('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example')}`,
  }))
})
