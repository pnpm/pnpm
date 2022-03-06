import { promises as fs } from 'fs'
import path from 'path'
import {
  addDependenciesToPackage,
  install,
  link,
  linkFromGlobal,
  linkToGlobal,
} from '@pnpm/core'
import fixtures from '@pnpm/test-fixtures'
import { prepareEmpty } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import { RootLog } from '@pnpm/core-loggers'
import { isExecutable } from '@pnpm/assert-project'
import exists from 'path-exists'
import sinon from 'sinon'
import writeJsonFile from 'write-json-file'
import symlink from 'symlink-dir'
import { testDefaults } from './utils'

const f = fixtures(__dirname)

test('relative link', async () => {
  const project = prepareEmpty()

  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgPath = path.resolve('..', linkedPkgName)

  f.copy(linkedPkgName, linkedPkgPath)
  await link([`../${linkedPkgName}`], path.join(process.cwd(), 'node_modules'), await testDefaults({
    dir: process.cwd(),
    manifest: {
      dependencies: {
        'hello-world-js-bin': '*',
      },
    },
  }))

  await project.isExecutable('.bin/hello-world-js-bin')

  const wantedLockfile = await project.readLockfile()
  expect(wantedLockfile.dependencies['hello-world-js-bin']).toBe('link:../hello-world-js-bin')
  expect(wantedLockfile.specifiers['hello-world-js-bin']).toBe('*')

  const currentLockfile = await project.readCurrentLockfile()
  expect(currentLockfile.dependencies['hello-world-js-bin']).toBe('link:../hello-world-js-bin')
})

test('relative link is linked by the name of the alias', async () => {
  const linkedPkgName = 'hello-world-js-bin'

  const project = prepareEmpty()

  const linkedPkgPath = path.resolve('..', linkedPkgName)

  f.copy(linkedPkgName, linkedPkgPath)
  await install({
    dependencies: {
      hello: `link:../${linkedPkgName}`,
    },
  }, await testDefaults())

  await project.isExecutable('.bin/hello-world-js-bin')

  await project.has('hello')

  const wantedLockfile = await project.readLockfile()
  expect(wantedLockfile.dependencies).toStrictEqual({
    hello: 'link:../hello-world-js-bin',
  })
})

test('relative link is not rewritten by argumentless install', async () => {
  const project = prepareEmpty()

  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgPath = path.resolve('..', linkedPkgName)

  const reporter = sinon.spy()
  const opts = await testDefaults()

  f.copy(linkedPkgName, linkedPkgPath)
  const manifest = await link(
    [linkedPkgPath],
    path.join(process.cwd(), 'node_modules'),
    {
      ...opts,
      dir: process.cwd(),
      manifest: {},
      reporter,
    }) // eslint-disable-line @typescript-eslint/no-explicit-any

  expect(reporter.calledWithMatch({
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
  } as RootLog)).toBeTruthy()

  await install(manifest, opts)

  expect(project.requireModule('hello-world-js-bin/package.json').isLocal).toBeTruthy()
})

test('relative link is rewritten by named installation to regular dependency', async () => {
  await addDistTag({ package: 'hello-world-js-bin', version: '1.0.0', distTag: 'latest' })
  const project = prepareEmpty()

  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgPath = path.resolve('..', linkedPkgName)

  const reporter = sinon.spy()
  const opts = await testDefaults({ fastUnpack: false })

  f.copy(linkedPkgName, linkedPkgPath)
  let manifest = await link(
    [linkedPkgPath],
    path.join(process.cwd(), 'node_modules'),
    {
      ...opts,
      dir: process.cwd(),
      manifest: {},
      reporter,
    }
  )

  expect(reporter.calledWithMatch({
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
  } as RootLog)).toBeTruthy()

  manifest = await addDependenciesToPackage(manifest, ['hello-world-js-bin'], opts)

  expect(manifest.dependencies).toStrictEqual({ 'hello-world-js-bin': '^1.0.0' })

  expect(project.requireModule('hello-world-js-bin/package.json').isLocal).toBeFalsy()

  const wantedLockfile = await project.readLockfile()
  expect(wantedLockfile.dependencies['hello-world-js-bin']).toBe('1.0.0')

  const currentLockfile = await project.readCurrentLockfile()
  expect(currentLockfile.dependencies['hello-world-js-bin']).toBe('1.0.0')
})

test('global link', async () => {
  const project = prepareEmpty()
  const projectPath = process.cwd()

  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgPath = path.resolve('..', linkedPkgName)

  f.copy(linkedPkgName, linkedPkgPath)

  const opts = await testDefaults()

  process.chdir(linkedPkgPath)
  const globalDir = path.resolve('..', 'global')
  const globalBin = path.resolve('..', 'global', 'bin')
  await linkToGlobal(process.cwd(), { ...opts, globalDir, globalBin, manifest: {} }) // eslint-disable-line @typescript-eslint/no-explicit-any

  await isExecutable((value, comment) => expect(value).toBeTruthy(), path.join(globalBin, 'hello-world-js-bin'))

  // bins of dependencies should not be linked, see issue https://github.com/pnpm/pnpm/issues/905
  expect(await exists(path.join(globalBin, 'cowsay'))).toBeFalsy() // cowsay not linked
  expect(await exists(path.join(globalBin, 'cowthink'))).toBeFalsy() // cowthink not linked

  process.chdir(projectPath)

  await linkFromGlobal([linkedPkgName], process.cwd(), { ...opts, globalDir, manifest: {} }) // eslint-disable-line @typescript-eslint/no-explicit-any

  await project.isExecutable('.bin/hello-world-js-bin')
})

test('failed linking should not create empty folder', async () => {
  prepareEmpty()

  const globalDir = path.resolve('..', 'global')

  try {
    await linkFromGlobal(['does-not-exist'], process.cwd(), await testDefaults({ globalDir, manifest: {} }))
    throw new Error('should have failed')
  } catch (err: any) { // eslint-disable-line
    expect(await exists(path.join(globalDir, 'node_modules', 'does-not-exist'))).toBeFalsy()
  }
})

test('node_modules is pruned after linking', async () => {
  prepareEmpty()

  await writeJsonFile('../is-positive/package.json', { name: 'is-positive', version: '1.0.0' })

  const manifest = await addDependenciesToPackage({}, ['is-positive@1.0.0'], await testDefaults())

  expect(await exists('node_modules/.pnpm/is-positive@1.0.0/node_modules/is-positive/package.json')).toBeTruthy()

  await link(['../is-positive'], path.resolve('node_modules'), await testDefaults({ manifest, dir: process.cwd() }))

  expect(await exists('node_modules/.pnpm/is-positive@1.0.0/node_modules/is-positive/package.json')).toBeFalsy()
})

test('relative link uses realpath when contained in a symlinked dir', async () => {
  prepareEmpty()

  // `process.cwd()` is now `.tmp/X/project`.

  f.copy('symlink-workspace', path.resolve('../symlink-workspace'))

  const app1RelPath = '../symlink-workspace/app1'
  const app2RelPath = '../symlink-workspace/app2'

  const app1 = path.resolve(app1RelPath)
  const app2 = path.resolve(app2RelPath)

  const dest = path.join(app2, 'packages/public')
  const src = path.resolve(app1, 'packages/public')

  console.log(`${dest}->${src}`)

  // We must manually create the symlink so it works in Windows too.
  await symlink(src, dest)

  process.chdir(path.join(app2, '/packages/public/foo'))

  // `process.cwd()` is now `.tmp/X/symlink-workspace/app2/packages/public/foo`.

  const linkFrom = path.join(app1, '/packages/public/bar')
  const linkTo = path.join(app2, '/packages/public/foo', 'node_modules')

  await link([linkFrom], linkTo, await testDefaults({ manifest: {}, dir: process.cwd() }))

  const linkToRelLink = await fs.readlink(path.join(linkTo, 'bar'))

  if (process.platform === 'win32') {
    expect(path.relative(linkToRelLink, path.join(src, 'bar'))).toBe('') // link points to real location
  } else {
    expect(linkToRelLink).toBe('../../bar')

    // If we don't use real paths we get a link like this.
    expect(linkToRelLink).not.toBe('../../../../../app1/packages/public/bar')
  }
})

test('throws error is package name is not defined', async () => {
  prepareEmpty()

  await writeJsonFile('../is-positive/package.json', { version: '1.0.0' })

  const manifest = await addDependenciesToPackage({}, ['is-positive@1.0.0'], await testDefaults())

  try {
    await link(['../is-positive'], path.resolve('node_modules'), await testDefaults({ manifest, dir: process.cwd() }))
    throw new Error('link package should fail')
  } catch (err: any) { // eslint-disable-line
    expect(err.message).toBe('Package in ../is-positive must have a name field to be linked')
    expect(err.code).toBe('ERR_PNPM_INVALID_PACKAGE_NAME')
  }
})

// test.skip('relative link when an external lockfile is used', async () => {
//   const projects = prepare(t, [
//     {
//       name: 'project',
//       version: '1.0.0',

//       dependencies: {},
//     },
//   ])

//   const opts = await testDefaults({ lockfileDir: path.join('..') })
//   await link([process.cwd()], path.resolve(process.cwd(), 'node_modules'), opts)

//   const lockfile = await readYamlFile<Lockfile>(path.resolve('..', WANTED_LOCKFILE))

//   t.deepEqual(lockfile && lockfile['importers'], {
//     project: {
//       dependencies: {
//         project: 'link:',
//       },
//       specifiers: {},
//     },
//   })
// })
