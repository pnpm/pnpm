import { promises as fs } from 'fs'
import path from 'path'
import {
  addDependenciesToPackage,
  install,
  link,
} from '@pnpm/core'
import { fixtures } from '@pnpm/test-fixtures'
import { prepareEmpty } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import { type RootLog } from '@pnpm/core-loggers'
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
        '@pnpm.e2e/hello-world-js-bin': '*',
      },
    },
  }))

  project.isExecutable('.bin/hello-world-js-bin')

  const wantedLockfile = project.readLockfile()
  expect(wantedLockfile.dependencies['@pnpm.e2e/hello-world-js-bin']).toStrictEqual({
    version: 'link:../hello-world-js-bin',
    specifier: '*',
  })

  const currentLockfile = project.readCurrentLockfile()
  expect(currentLockfile.dependencies['@pnpm.e2e/hello-world-js-bin']).toStrictEqual({
    version: 'link:../hello-world-js-bin',
    specifier: '*',
  })
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

  project.isExecutable('.bin/hello-world-js-bin')

  project.has('hello')

  const wantedLockfile = project.readLockfile()
  expect(wantedLockfile.dependencies).toStrictEqual({
    hello: {
      specifier: 'link:../hello-world-js-bin',
      version: 'link:../hello-world-js-bin',
    },
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
    })

  expect(reporter.calledWithMatch({
    added: {
      dependencyType: undefined,
      linkedFrom: linkedPkgPath,
      name: '@pnpm.e2e/hello-world-js-bin',
      realName: '@pnpm.e2e/hello-world-js-bin',
      version: '1.0.0',
    },
    level: 'debug',
    name: 'pnpm:root',
    prefix: process.cwd(),
  } as RootLog)).toBeTruthy()

  await install(manifest, opts)

  expect(project.requireModule('@pnpm.e2e/hello-world-js-bin/package.json').isLocal).toBeTruthy()
})

test('relative link is rewritten by named installation to regular dependency', async () => {
  await addDistTag({ package: '@pnpm.e2e/hello-world-js-bin', version: '1.0.0', distTag: 'latest' })
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
      name: '@pnpm.e2e/hello-world-js-bin',
      realName: '@pnpm.e2e/hello-world-js-bin',
      version: '1.0.0',
    },
    level: 'debug',
    name: 'pnpm:root',
    prefix: process.cwd(),
  } as RootLog)).toBeTruthy()

  manifest = await addDependenciesToPackage(manifest, ['@pnpm.e2e/hello-world-js-bin'], opts)

  expect(manifest.dependencies).toStrictEqual({ '@pnpm.e2e/hello-world-js-bin': '^1.0.0' })

  expect(project.requireModule('@pnpm.e2e/hello-world-js-bin/package.json').isLocal).toBeFalsy()

  const wantedLockfile = project.readLockfile()
  expect(wantedLockfile.dependencies['@pnpm.e2e/hello-world-js-bin'].version).toBe('1.0.0')

  const currentLockfile = project.readCurrentLockfile()
  expect(currentLockfile.dependencies['@pnpm.e2e/hello-world-js-bin'].version).toBe('1.0.0')
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

test('link should not change the type of the dependency', async () => {
  const project = prepareEmpty()

  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgPath = path.resolve('..', linkedPkgName)

  f.copy(linkedPkgName, linkedPkgPath)
  await link([`../${linkedPkgName}`], path.join(process.cwd(), 'node_modules'), await testDefaults({
    dir: process.cwd(),
    manifest: {
      devDependencies: {
        '@pnpm.e2e/hello-world-js-bin': '*',
      },
    },
  }))

  project.isExecutable('.bin/hello-world-js-bin')

  const wantedLockfile = project.readLockfile()
  expect(wantedLockfile.devDependencies).toStrictEqual({
    '@pnpm.e2e/hello-world-js-bin': {
      version: 'link:../hello-world-js-bin',
      specifier: '*',
    },
  })
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
