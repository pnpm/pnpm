import * as path from 'path'
import {
  addDependenciesToPackage,
  install,
  mutateModules,
} from 'supi'
import { prepareEmpty } from '@pnpm/prepare'
import { LifecycleLog } from '@pnpm/core-loggers'
import { testDefaults } from '../utils'
import rimraf = require('@zkochan/rimraf')
import loadJsonFile = require('load-json-file')
import fs = require('mz/fs')
import exists = require('path-exists')
import PATH = require('path-name')
import sinon = require('sinon')

test('run pre/postinstall scripts', async () => {
  const project = prepareEmpty()
  const manifest = await addDependenciesToPackage({},
    ['pre-and-postinstall-scripts-example'],
    await testDefaults({ fastUnpack: false, targetDependenciesField: 'devDependencies' })
  )

  {
    expect(await exists('node_modules/pre-and-postinstall-scripts-example/generated-by-prepare.js')).toBeFalsy()
    expect(await exists('node_modules/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeTruthy()

    const generatedByPreinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-preinstall')
    expect(typeof generatedByPreinstall).toBe('function')

    const generatedByPostinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-postinstall')
    expect(typeof generatedByPostinstall).toBe('function')
  }

  await rimraf('node_modules')

  // testing that the packages are not installed even though they are in lockfile
  // and that their scripts are not tried to be executed

  await install(manifest, await testDefaults({ fastUnpack: false, production: true }))

  {
    const generatedByPreinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-preinstall')
    expect(typeof generatedByPreinstall).toBe('function')

    const generatedByPostinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-postinstall')
    expect(typeof generatedByPostinstall).toBe('function')
  }

  const lockfile = await project.readLockfile()
  expect(lockfile.packages['/pre-and-postinstall-scripts-example/1.0.0'].requiresBuild)
})

test('run pre/postinstall scripts, when PnP is used and no symlinks', async () => {
  prepareEmpty()
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
  expect(await exists(path.resolve(pkgDir, 'generated-by-prepare.js'))).toBeFalsy()
  expect(await exists(path.resolve(pkgDir, 'generated-by-preinstall.js'))).toBeTruthy()
  expect(await exists(path.resolve(pkgDir, 'generated-by-postinstall.js'))).toBeTruthy()
})

test('testing that the bins are linked when the package with the bins was already in node_modules', async () => {
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['hello-world-js-bin'], await testDefaults({ fastUnpack: false }))
  await addDependenciesToPackage(manifest, ['pre-and-postinstall-scripts-example'], await testDefaults({ fastUnpack: false, targetDependenciesField: 'devDependencies' }))

  const generatedByPreinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-preinstall')
  expect(typeof generatedByPreinstall).toBe('function')

  const generatedByPostinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-postinstall')
  expect(typeof generatedByPostinstall).toBe('function')
})

test('run install scripts', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['install-script-example'], await testDefaults({ fastUnpack: false }))

  const generatedByInstall = project.requireModule('install-script-example/generated-by-install')
  expect(typeof generatedByInstall).toBe('function')
})

test('run install scripts in the current project', async () => {
  prepareEmpty()
  const manifest = await addDependenciesToPackage({
    scripts: {
      install: 'node -e "process.stdout.write(\'install\')" | json-append output.json',
      postinstall: 'node -e "process.stdout.write(\'postinstall\')" | json-append output.json',
      preinstall: 'node -e "process.stdout.write(\'preinstall\')" | json-append output.json',
    },
  }, ['json-append@1.1.1'], await testDefaults({ fastUnpack: false }))
  await install(manifest, await testDefaults({ fastUnpack: false }))

  const output = await loadJsonFile<string[]>('output.json')

  expect(output).toStrictEqual(['preinstall', 'install', 'postinstall'])
})

test('run install scripts in the current project when its name is different than its directory', async () => {
  prepareEmpty()
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

  expect(output).toStrictEqual(['preinstall', 'install', 'postinstall'])
})

test('do not run install scripts if unsafePerm is false', async () => {
  prepareEmpty()
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

  expect(outputExists).toBeFalsy()
})

test('installation fails if lifecycle script fails', async () => {
  prepareEmpty()

  await expect(
    install({
      scripts: {
        preinstall: 'exit 1',
      },
    }, await testDefaults({ fastUnpack: false }))
  ).rejects.toThrow(/@ preinstall: `exit 1`/)
})

test('INIT_CWD is always set to lockfile directory', async () => {
  prepareEmpty()
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
  expect(childEnv.INIT_CWD).toBe(rootDir)

  const output = await loadJsonFile(path.join(rootDir, 'output.json'))
  expect(output).toStrictEqual([process.cwd()])
})

// TODO: duplicate this test to @pnpm/lifecycle
test("reports child's output", async () => {
  prepareEmpty()

  const reporter = sinon.spy()

  await addDependenciesToPackage({}, ['count-to-10'], await testDefaults({ fastUnpack: false, reporter }))

  expect(reporter.calledWithMatch({
    depPath: '/count-to-10/1.0.0',
    level: 'debug',
    name: 'pnpm:lifecycle',
    script: 'node postinstall',
    stage: 'postinstall',
  } as LifecycleLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    depPath: '/count-to-10/1.0.0',
    level: 'debug',
    line: '1',
    name: 'pnpm:lifecycle',
    stage: 'postinstall',
    stdio: 'stdout',
  } as LifecycleLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    depPath: '/count-to-10/1.0.0',
    level: 'debug',
    line: '2',
    name: 'pnpm:lifecycle',
    stage: 'postinstall',
    stdio: 'stdout',
  } as LifecycleLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    depPath: '/count-to-10/1.0.0',
    level: 'debug',
    line: '6',
    name: 'pnpm:lifecycle',
    stage: 'postinstall',
    stdio: 'stderr',
  } as LifecycleLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    depPath: '/count-to-10/1.0.0',
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
    addDependenciesToPackage({}, ['failing-postinstall'], await testDefaults({ reporter }))
  ).rejects.toThrow()

  expect(reporter.calledWithMatch({
    depPath: '/failing-postinstall/1.0.0',
    exitCode: 1,
    level: 'debug',
    name: 'pnpm:lifecycle',
    stage: 'postinstall',
  } as LifecycleLog)).toBeTruthy()
})

test('lifecycle scripts have access to node-gyp', async () => {
  prepareEmpty()

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
})

test('run lifecycle scripts of dependent packages after running scripts of their deps', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['with-postinstall-a'], await testDefaults({ fastUnpack: false }))

  expect(+project.requireModule('.pnpm/with-postinstall-b@1.0.0/node_modules/with-postinstall-b/output.json')[0] < +project.requireModule('with-postinstall-a/output.json')[0]).toBeTruthy()
})

test('run prepare script for git-hosted dependencies', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['zkochan/install-scripts-example#prepare'], await testDefaults({ fastUnpack: false }))

  const scripts = project.requireModule('install-scripts-example-for-pnpm/output.json')
  expect(scripts).toStrictEqual([
    'preinstall',
    'install',
    'postinstall',
    'prepare',
  ])

  const lockfile = await project.readLockfile()
  expect(lockfile.packages['github.com/zkochan/install-scripts-example/2de638b8b572cd1e87b74f4540754145fb2c0ebb'].prepare === true).toBeTruthy()
})

test('lifecycle scripts run before linking bins', async () => {
  const project = prepareEmpty()

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

test('hoisting does not fail on commands that will be created by lifecycle scripts on a later stage', async () => {
  prepareEmpty()

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
})

test('bins are linked even if lifecycle scripts are ignored', async () => {
  const project = prepareEmpty()

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
  expect(await exists('node_modules/pre-and-postinstall-scripts-example/package.json')).toBeTruthy()
  expect(await exists('node_modules/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeFalsy()

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
  expect(await exists('node_modules/pre-and-postinstall-scripts-example/package.json')).toBeTruthy()
  expect(await exists('node_modules/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeFalsy()
})

test('dependency should not be added to current lockfile if it was not built successfully during headless install', async () => {
  const project = prepareEmpty()

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

  await expect(
    mutateModules(
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
  ).rejects.toThrow()

  expect(await project.readCurrentLockfile()).toBeFalsy()
})

test('scripts have access to unlisted bins when hoisting is used', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage(
    {},
    ['pkg-that-calls-unlisted-dep-in-hooks'],
    await testDefaults({ fastUnpack: false, hoistPattern: '*' })
  )

  expect(project.requireModule('pkg-that-calls-unlisted-dep-in-hooks/output.json')).toStrictEqual(['Hello world!'])
})

test('selectively ignore scripts in some dependencies', async () => {
  const project = prepareEmpty()
  const neverBuiltDependencies = ['pre-and-postinstall-scripts-example']
  const manifest = await addDependenciesToPackage({ pnpm: { neverBuiltDependencies } },
    ['pre-and-postinstall-scripts-example', 'install-script-example'],
    await testDefaults({ fastUnpack: false })
  )

  expect(await exists('node_modules/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeFalsy()
  expect(await exists('node_modules/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeFalsy()
  expect(await exists('node_modules/install-script-example/generated-by-install.js')).toBeTruthy()

  const lockfile = await project.readLockfile()
  expect(lockfile.neverBuiltDependencies).toStrictEqual(neverBuiltDependencies)
  expect(lockfile.packages['/pre-and-postinstall-scripts-example/1.0.0'].requiresBuild).toBe(undefined)
  expect(lockfile.packages['/install-script-example/1.0.0'].requiresBuild).toBeTruthy()

  await rimraf('node_modules')

  await install(manifest, await testDefaults({ fastUnpack: false, frozenLockfile: true }))

  expect(await exists('node_modules/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeFalsy()
  expect(await exists('node_modules/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeFalsy()
  expect(await exists('node_modules/install-script-example/generated-by-install.js')).toBeTruthy()
})

test('lockfile is updated if neverBuiltDependencies is changed', async () => {
  const project = prepareEmpty()
  const manifest = await addDependenciesToPackage({},
    ['pre-and-postinstall-scripts-example', 'install-script-example'],
    await testDefaults({ fastUnpack: false })
  )

  {
    const lockfile = await project.readLockfile()
    expect(lockfile.neverBuiltDependencies).toBeFalsy()
    expect(lockfile.packages['/pre-and-postinstall-scripts-example/1.0.0'].requiresBuild).toBeTruthy()
    expect(lockfile.packages['/install-script-example/1.0.0'].requiresBuild).toBeTruthy()
  }

  const neverBuiltDependencies = ['pre-and-postinstall-scripts-example']
  manifest.pnpm = { neverBuiltDependencies }
  await mutateModules([
    {
      buildIndex: 0,
      manifest,
      mutation: 'install',
      rootDir: process.cwd(),
    },
  ], await testDefaults())

  {
    const lockfile = await project.readLockfile()
    expect(lockfile.neverBuiltDependencies).toStrictEqual(neverBuiltDependencies)
    expect(lockfile.packages['/pre-and-postinstall-scripts-example/1.0.0'].requiresBuild).toBe(undefined)
    expect(lockfile.packages['/install-script-example/1.0.0'].requiresBuild).toBeTruthy()
  }
})
