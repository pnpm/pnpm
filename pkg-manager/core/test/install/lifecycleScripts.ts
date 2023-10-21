import * as path from 'path'
import { promises as fs } from 'fs'
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
import { restartWorkerPool } from '@pnpm/worker'
import rimraf from '@zkochan/rimraf'
import isWindows from 'is-windows'
import loadJsonFile from 'load-json-file'
import exists from 'path-exists'
import PATH from 'path-name'
import sinon from 'sinon'
import { testDefaults } from '../utils'

const testOnNonWindows = isWindows() ? test.skip : test

test('run pre/postinstall scripts', async () => {
  const project = prepareEmpty()
  const manifest = await addDependenciesToPackage({},
    ['@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0'],
    await testDefaults({ fastUnpack: false, targetDependenciesField: 'devDependencies' })
  )

  {
    expect(await exists('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-prepare.js')).toBeFalsy()
    expect(await exists('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeTruthy()

    const generatedByPreinstall = project.requireModule('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall')
    expect(typeof generatedByPreinstall).toBe('function')

    const generatedByPostinstall = project.requireModule('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall')
    expect(typeof generatedByPostinstall).toBe('function')
  }

  await rimraf('node_modules')

  // testing that the packages are not installed even though they are in lockfile
  // and that their scripts are not tried to be executed

  await install(manifest, await testDefaults({ fastUnpack: false, production: true }))

  {
    const generatedByPreinstall = project.requireModule('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall')
    expect(typeof generatedByPreinstall).toBe('function')

    const generatedByPostinstall = project.requireModule('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall')
    expect(typeof generatedByPostinstall).toBe('function')
  }

  const lockfile = await project.readLockfile()
  expect(lockfile.packages['/@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0'].requiresBuild)
})

test('run pre/postinstall scripts, when PnP is used and no symlinks', async () => {
  prepareEmpty()
  await addDependenciesToPackage({},
    ['@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0'],
    await testDefaults({
      fastUnpack: false,
      enablePnp: true,
      symlink: false,
      targetDependenciesField: 'devDependencies',
    })
  )

  const pkgDir = 'node_modules/.pnpm/@pnpm.e2e+pre-and-postinstall-scripts-example@1.0.0/node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example'
  expect(await exists(path.resolve(pkgDir, 'generated-by-prepare.js'))).toBeFalsy()
  expect(await exists(path.resolve(pkgDir, 'generated-by-preinstall.js'))).toBeTruthy()
  expect(await exists(path.resolve(pkgDir, 'generated-by-postinstall.js'))).toBeTruthy()
})

test('testing that the bins are linked when the package with the bins was already in node_modules', async () => {
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/hello-world-js-bin'], await testDefaults({ fastUnpack: false }))
  await addDependenciesToPackage(manifest, ['@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0'], await testDefaults({ fastUnpack: false, targetDependenciesField: 'devDependencies' }))

  const generatedByPreinstall = project.requireModule('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall')
  expect(typeof generatedByPreinstall).toBe('function')

  const generatedByPostinstall = project.requireModule('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall')
  expect(typeof generatedByPostinstall).toBe('function')
})

test('run install scripts', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['@pnpm.e2e/install-script-example'], await testDefaults({ fastUnpack: false }))

  const generatedByInstall = project.requireModule('@pnpm.e2e/install-script-example/generated-by-install')
  expect(typeof generatedByInstall).toBe('function')
})

test('run install scripts in the current project', async () => {
  prepareEmpty()
  const manifest = await addDependenciesToPackage({
    scripts: {
      'pnpm:devPreinstall': 'node -e "require(\'fs\').writeFileSync(\'test.txt\', \'\', \'utf-8\')"',
      install: 'node -e "process.stdout.write(\'install\')" | json-append output.json',
      postinstall: 'node -e "process.stdout.write(\'postinstall\')" | json-append output.json',
      preinstall: 'node -e "process.stdout.write(\'preinstall\')" | json-append output.json',
    },
  }, ['json-append@1.1.1'], await testDefaults({ fastUnpack: false }))
  await install(manifest, await testDefaults({ fastUnpack: false }))

  const output = await loadJsonFile<string[]>('output.json')

  expect(output).toStrictEqual(['preinstall', 'install', 'postinstall'])
  expect(await exists('test.txt')).toBeTruthy()
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
  await fs.mkdir('sub_dir')
  process.chdir('sub_dir')
  await mutateModulesInSingleProject({
    mutation: 'install',
    manifest: {
      dependencies: {
        'json-append': '1.1.1',
        '@pnpm.e2e/write-lifecycle-env': '1.0.0',
      },
      scripts: {
        install: 'node -e "process.stdout.write(process.env.INIT_CWD)" | json-append output.json',
      },
    },
    rootDir,
  }, await testDefaults({
    fastUnpack: false,
    lockfileDir: rootDir,
  }))

  const childEnv = await loadJsonFile<{ INIT_CWD: string }>(path.join(rootDir, 'node_modules/@pnpm.e2e/write-lifecycle-env/env.json'))
  expect(childEnv.INIT_CWD).toBe(rootDir)

  const output = await loadJsonFile(path.join(rootDir, 'output.json'))
  expect(output).toStrictEqual([process.cwd()])
})

// TODO: duplicate this test to @pnpm/lifecycle
test("reports child's output", async () => {
  prepareEmpty()

  const reporter = sinon.spy()

  await addDependenciesToPackage({}, ['@pnpm.e2e/count-to-10'], await testDefaults({ fastUnpack: false, reporter }))

  expect(reporter.calledWithMatch({
    depPath: '/@pnpm.e2e/count-to-10/1.0.0',
    level: 'debug',
    name: 'pnpm:lifecycle',
    script: 'node postinstall',
    stage: 'postinstall',
  } as LifecycleLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    depPath: '/@pnpm.e2e/count-to-10/1.0.0',
    level: 'debug',
    line: '1',
    name: 'pnpm:lifecycle',
    stage: 'postinstall',
    stdio: 'stdout',
  } as LifecycleLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    depPath: '/@pnpm.e2e/count-to-10/1.0.0',
    level: 'debug',
    line: '2',
    name: 'pnpm:lifecycle',
    stage: 'postinstall',
    stdio: 'stdout',
  } as LifecycleLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    depPath: '/@pnpm.e2e/count-to-10/1.0.0',
    level: 'debug',
    line: '6',
    name: 'pnpm:lifecycle',
    stage: 'postinstall',
    stdio: 'stderr',
  } as LifecycleLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    depPath: '/@pnpm.e2e/count-to-10/1.0.0',
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
    addDependenciesToPackage({}, ['@pnpm.e2e/failing-postinstall'], await testDefaults({ reporter }))
  ).rejects.toThrow()

  expect(reporter.calledWithMatch({
    depPath: '/@pnpm.e2e/failing-postinstall/1.0.0',
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

  await addDependenciesToPackage({}, ['drivelist@5.1.8'], await testDefaults({ fastUnpack: false }))

  process.env[PATH] = initialPath
})

test('run lifecycle scripts of dependent packages after running scripts of their deps', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['@pnpm.e2e/with-postinstall-a'], await testDefaults({ fastUnpack: false }))

  expect(+project.requireModule('.pnpm/@pnpm.e2e+with-postinstall-b@1.0.0/node_modules/@pnpm.e2e/with-postinstall-b/output.json')[0] < +project.requireModule('@pnpm.e2e/with-postinstall-a/output.json')[0]).toBeTruthy()
})

test('run prepare script for git-hosted dependencies', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['pnpm/test-git-fetch#d222f6bfbdea55c032fdb5f0538d52b2a484bbbf'], await testDefaults({ fastUnpack: false }))

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

  const lockfile = await project.readLockfile()
  expect(lockfile.packages['github.com/pnpm/test-git-fetch/d222f6bfbdea55c032fdb5f0538d52b2a484bbbf'].prepare === true).toBeTruthy()
})

test('lifecycle scripts run before linking bins', async () => {
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/generated-bins'], await testDefaults({ fastUnpack: false }))

  await project.isExecutable('.bin/cmd1')
  await project.isExecutable('.bin/cmd2')

  await rimraf('node_modules')

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd(),
  }, await testDefaults({ frozenLockfile: true }))

  await project.isExecutable('.bin/cmd1')
  await project.isExecutable('.bin/cmd2')
})

test('hoisting does not fail on commands that will be created by lifecycle scripts on a later stage', async () => {
  prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/has-generated-bins-as-dep'], await testDefaults({ fastUnpack: false, hoistPattern: '*' }))

  // await project.isExecutable('.pnpm/node_modules/.bin/cmd1')
  // await project.isExecutable('.pnpm/node_modules/.bin/cmd2')

  // Testing the same with headless installation
  await rimraf('node_modules')

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd(),
  }, await testDefaults({ frozenLockfile: true, hoistPattern: '*' }))

  // await project.isExecutable('.pnpm/node_modules/.bin/cmd1')
  // await project.isExecutable('.pnpm/node_modules/.bin/cmd2')
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
    await testDefaults({ fastUnpack: false, ignoreScripts: true })
  )

  await project.isExecutable('.bin/peer-with-bin')
  await project.isExecutable('@pnpm.e2e/pkg-with-peer-having-bin/node_modules/.bin/hello-world-js-bin')

  // Verifying that the scripts were ignored
  expect(await exists('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/package.json')).toBeTruthy()
  expect(await exists('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeFalsy()

  await rimraf('node_modules')

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd(),
  }, await testDefaults({ frozenLockfile: true, ignoreScripts: true }))

  await project.isExecutable('.bin/peer-with-bin')
  await project.isExecutable('@pnpm.e2e/pkg-with-peer-having-bin/node_modules/.bin/hello-world-js-bin')

  // Verifying that the scripts were ignored
  expect(await exists('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/package.json')).toBeTruthy()
  expect(await exists('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeFalsy()
})

test('dependency should not be added to current lockfile if it was not built successfully during headless install', async () => {
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage(
    {},
    [
      'package-that-cannot-be-installed@0.0.0', // TODO: this package should be replaced
    ],
    await testDefaults({
      ignoreScripts: true,
      lockfileOnly: true,
    })
  )

  await expect(
    mutateModulesInSingleProject({
      manifest,
      mutation: 'install',
      rootDir: process.cwd(),
    }, await testDefaults({ frozenLockfile: true }))
  ).rejects.toThrow()

  expect(await project.readCurrentLockfile()).toBeFalsy()
})

test('scripts have access to unlisted bins when hoisting is used', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage(
    {},
    ['@pnpm.e2e/pkg-that-calls-unlisted-dep-in-hooks'],
    await testDefaults({ fastUnpack: false, hoistPattern: '*' })
  )

  expect(project.requireModule('@pnpm.e2e/pkg-that-calls-unlisted-dep-in-hooks/output.json')).toStrictEqual(['Hello world!'])
})

test('selectively ignore scripts in some dependencies by neverBuiltDependencies', async () => {
  const project = prepareEmpty()
  const neverBuiltDependencies = ['@pnpm.e2e/pre-and-postinstall-scripts-example']
  const manifest = await addDependenciesToPackage({},
    ['@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0', '@pnpm.e2e/install-script-example'],
    await testDefaults({ fastUnpack: false, neverBuiltDependencies })
  )

  expect(await exists('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeFalsy()
  expect(await exists('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeFalsy()
  expect(await exists('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeTruthy()

  const lockfile = await project.readLockfile()
  expect(lockfile.neverBuiltDependencies).toStrictEqual(neverBuiltDependencies)
  expect(lockfile.packages['/@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0'].requiresBuild).toBe(undefined)
  expect(lockfile.packages['/@pnpm.e2e/install-script-example@1.0.0'].requiresBuild).toBeTruthy()

  await rimraf('node_modules')

  await install(manifest, await testDefaults({ fastUnpack: false, frozenLockfile: true, neverBuiltDependencies }))

  expect(await exists('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeFalsy()
  expect(await exists('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeFalsy()
  expect(await exists('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeTruthy()
})

test('throw an exception when both neverBuiltDependencies and onlyBuiltDependencies are used', async () => {
  prepareEmpty()

  await expect(
    addDependenciesToPackage(
      {},
      ['@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0'],
      await testDefaults({ onlyBuiltDependencies: ['@pnpm.e2e/foo'], neverBuiltDependencies: ['@pnpm.e2e/bar'] })
    )
  ).rejects.toThrow(/Cannot have both/)
})

test('selectively allow scripts in some dependencies by onlyBuiltDependencies', async () => {
  const project = prepareEmpty()
  const onlyBuiltDependencies = ['@pnpm.e2e/install-script-example']
  const manifest = await addDependenciesToPackage({},
    ['@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0', '@pnpm.e2e/install-script-example'],
    await testDefaults({ fastUnpack: false, onlyBuiltDependencies })
  )

  expect(await exists('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeFalsy()
  expect(await exists('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeFalsy()
  expect(await exists('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeTruthy()

  const lockfile = await project.readLockfile()
  expect(lockfile.onlyBuiltDependencies).toStrictEqual(onlyBuiltDependencies)
  expect(lockfile.packages['/@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0'].requiresBuild).toBe(undefined)
  expect(lockfile.packages['/@pnpm.e2e/install-script-example@1.0.0'].requiresBuild).toBe(true)

  await rimraf('node_modules')

  await install(manifest, await testDefaults({ fastUnpack: false, frozenLockfile: true, onlyBuiltDependencies }))

  expect(await exists('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeFalsy()
  expect(await exists('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeFalsy()
  expect(await exists('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeTruthy()
})

test('selectively allow scripts in some dependencies by onlyBuiltDependenciesFile', async () => {
  prepareEmpty()
  const onlyBuiltDependenciesFile = path.resolve('node_modules/@pnpm.e2e/build-allow-list/list.json')
  const manifest = await addDependenciesToPackage({},
    ['@pnpm.e2e/build-allow-list', '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0', '@pnpm.e2e/install-script-example'],
    await testDefaults({ fastUnpack: false, onlyBuiltDependenciesFile })
  )

  expect(await exists('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeFalsy()
  expect(await exists('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeFalsy()
  expect(await exists('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeTruthy()

  await rimraf('node_modules')

  await install(manifest, await testDefaults({ fastUnpack: false, frozenLockfile: true, onlyBuiltDependenciesFile }))

  expect(await exists('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeFalsy()
  expect(await exists('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeFalsy()
  expect(await exists('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeTruthy()
})

test('selectively allow scripts in some dependencies by onlyBuiltDependenciesFile and onlyBuiltDependencies', async () => {
  prepareEmpty()
  const onlyBuiltDependenciesFile = path.resolve('node_modules/@pnpm.e2e/build-allow-list/list.json')
  const onlyBuiltDependencies = ['@pnpm.e2e/pre-and-postinstall-scripts-example']
  const manifest = await addDependenciesToPackage({},
    ['@pnpm.e2e/build-allow-list', '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0', '@pnpm.e2e/install-script-example'],
    await testDefaults({ fastUnpack: false, onlyBuiltDependenciesFile, onlyBuiltDependencies })
  )

  expect(await exists('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeTruthy()
  expect(await exists('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeTruthy()
  expect(await exists('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeTruthy()

  await rimraf('node_modules')

  await install(manifest, await testDefaults({ fastUnpack: false, frozenLockfile: true, onlyBuiltDependenciesFile, onlyBuiltDependencies }))

  expect(await exists('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeTruthy()
  expect(await exists('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeTruthy()
  expect(await exists('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeTruthy()
})

test('lockfile is updated if neverBuiltDependencies is changed', async () => {
  const project = prepareEmpty()
  const manifest = await addDependenciesToPackage({},
    ['@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0', '@pnpm.e2e/install-script-example'],
    await testDefaults({ fastUnpack: false })
  )

  {
    const lockfile = await project.readLockfile()
    expect(lockfile.neverBuiltDependencies).toBeFalsy()
    expect(lockfile.packages['/@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0'].requiresBuild).toBeTruthy()
    expect(lockfile.packages['/@pnpm.e2e/install-script-example@1.0.0'].requiresBuild).toBeTruthy()
  }

  const neverBuiltDependencies = ['@pnpm.e2e/pre-and-postinstall-scripts-example']
  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd(),
  }, await testDefaults({ neverBuiltDependencies }))

  {
    const lockfile = await project.readLockfile()
    expect(lockfile.neverBuiltDependencies).toStrictEqual(neverBuiltDependencies)
    expect(lockfile.packages['/@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0'].requiresBuild).toBe(undefined)
    expect(lockfile.packages['/@pnpm.e2e/install-script-example@1.0.0'].requiresBuild).toBeTruthy()
  }
})

test('lockfile is updated if onlyBuiltDependencies is changed', async () => {
  const project = prepareEmpty()
  const manifest = await addDependenciesToPackage({},
    ['@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0', '@pnpm.e2e/install-script-example'],
    await testDefaults({ fastUnpack: false })
  )

  {
    const lockfile = await project.readLockfile()
    expect(lockfile.onlyBuiltDependencies).toBeFalsy()
    expect(lockfile.packages['/@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0'].requiresBuild).toBeTruthy()
    expect(lockfile.packages['/@pnpm.e2e/install-script-example@1.0.0'].requiresBuild).toBeTruthy()
  }

  const onlyBuiltDependencies: string[] = []
  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd(),
  }, await testDefaults({ onlyBuiltDependencies }))

  {
    const lockfile = await project.readLockfile()
    expect(lockfile.onlyBuiltDependencies).toStrictEqual(onlyBuiltDependencies)
    expect(lockfile.packages['/@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0'].requiresBuild).toBe(undefined)
    expect(lockfile.packages['/@pnpm.e2e/install-script-example@1.0.0'].requiresBuild).toBe(undefined)
  }

  onlyBuiltDependencies.push('@pnpm.e2e/pre-and-postinstall-scripts-example')
  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd(),
  }, await testDefaults({ onlyBuiltDependencies }))

  {
    const lockfile = await project.readLockfile()
    expect(lockfile.onlyBuiltDependencies).toStrictEqual(onlyBuiltDependencies)
    expect(lockfile.packages['/@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0'].requiresBuild).toBe(true)
    expect(lockfile.packages['/@pnpm.e2e/install-script-example@1.0.0'].requiresBuild).toBe(undefined)
  }
})

test('lifecycle scripts have access to package\'s own binary by binary name', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({},
    ['@pnpm.e2e/runs-own-bin'],
    await testDefaults({ fastUnpack: false })
  )

  await project.isExecutable('.pnpm/@pnpm.e2e+runs-own-bin@1.0.0/node_modules/@pnpm.e2e/runs-own-bin/node_modules/.bin/runs-own-bin')
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
    rootDir: process.cwd(),
  }, await testDefaults({ fastUnpack: false }))

  await rimraf('node_modules')

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd(),
  }, await testDefaults({ fastUnpack: false, frozenLockfile: true }))

  // if there was no exception, the test passed
})

test('ignore-dep-scripts', async () => {
  prepareEmpty()
  const manifest = {
    scripts: {
      'pnpm:devPreinstall': 'node -e "require(\'fs\').writeFileSync(\'test.txt\', \'\', \'utf-8\')"',
      install: 'node -e "process.stdout.write(\'install\')" | json-append output.json',
      postinstall: 'node -e "process.stdout.write(\'postinstall\')" | json-append output.json',
      preinstall: 'node -e "process.stdout.write(\'preinstall\')" | json-append output.json',
    },
    dependencies: {
      'json-append': '1.1.1',
      '@pnpm.e2e/pre-and-postinstall-scripts-example': '1.0.0',
    },
  }
  await install(manifest, await testDefaults({ fastUnpack: false, ignoreDepScripts: true }))

  {
    const output = await loadJsonFile<string[]>('output.json')

    expect(output).toStrictEqual(['preinstall', 'install', 'postinstall'])
    expect(await exists('test.txt')).toBeTruthy()

    expect(await exists('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeFalsy()
  }

  await rimraf('node_modules')
  await rimraf('output.json')
  await install(manifest, await testDefaults({ fastUnpack: false, ignoreDepScripts: true }))
  {
    const output = await loadJsonFile<string[]>('output.json')

    expect(output).toStrictEqual(['preinstall', 'install', 'postinstall'])
    expect(await exists('test.txt')).toBeTruthy()

    expect(await exists('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeFalsy()
  }
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
      rootDir: path.resolve('project-1'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-3'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-4'),
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
      rootDir: path.resolve('project-1'),
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
      rootDir: path.resolve('project-2'),
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
      rootDir: path.resolve('project-3'),
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
      rootDir: path.resolve('project-4'),
    },
  ]
  await mutateModules(importers, await testDefaults({
    allProjects,
    fastUnpack: false,
    nodeLinker: 'hoisted',
  }))
  const rootProject = assertProject(process.cwd())
  await rootProject.has('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  await rootProject.has('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')
  await projects['project-1'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  await projects['project-1'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')
  await projects['project-2'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  await projects['project-2'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')
  await projects['project-3'].has('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  await projects['project-3'].has('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')
  await projects['project-4'].has('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  await projects['project-4'].has('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')
})

test('run pre/postinstall scripts in a project that uses node-linker=hoisted. Should not fail on repeat install', async () => {
  const project = prepareEmpty()
  const manifest = await addDependenciesToPackage({},
    ['@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0'],
    await testDefaults({ fastUnpack: false, targetDependenciesField: 'devDependencies', nodeLinker: 'hoisted', sideEffectsCacheRead: true, sideEffectsCacheWrite: true })
  )

  {
    expect(await exists('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-prepare.js')).toBeFalsy()
    expect(await exists('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeTruthy()

    const generatedByPreinstall = project.requireModule('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall')
    expect(typeof generatedByPreinstall).toBe('function')

    const generatedByPostinstall = project.requireModule('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall')
    expect(typeof generatedByPostinstall).toBe('function')
  }

  const reporter = jest.fn()
  await addDependenciesToPackage(manifest,
    ['example@npm:@pnpm.e2e/pre-and-postinstall-scripts-example@2.0.0'],
    await testDefaults({
      fastUnpack: false,
      targetDependenciesField: 'devDependencies',
      nodeLinker: 'hoisted',
      reporter,
      sideEffectsCacheRead: true,
      sideEffectsCacheWrite: true,
    })
  )

  expect(reporter).not.toBeCalledWith(expect.objectContaining({
    level: 'warn',
    message: `An error occurred while uploading ${path.resolve('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example')}`,
  }))
})
