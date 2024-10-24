import path from 'path'
import { addDependenciesToPackage, install } from '@pnpm/core'
import { fixtures } from '@pnpm/test-fixtures'
import { prepareEmpty } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import symlinkDir from 'symlink-dir'
import { testDefaults } from './utils'

const f = fixtures(__dirname)

test('relative link is linked by the name of the alias', async () => {
  const linkedPkgName = 'hello-world-js-bin'

  const project = prepareEmpty()

  const linkedPkgPath = path.resolve('..', linkedPkgName)

  f.copy(linkedPkgName, linkedPkgPath)
  await install({
    dependencies: {
      hello: `link:../${linkedPkgName}`,
    },
  }, testDefaults())

  project.isExecutable('.bin/hello-world-js-bin')

  project.has('hello')

  const wantedLockfile = project.readLockfile()
  expect(wantedLockfile.importers['.'].dependencies).toStrictEqual({
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

  f.copy(linkedPkgName, linkedPkgPath)
  symlinkDir.sync(linkedPkgPath, path.resolve('node_modules/@pnpm.e2e/hello-world-js-bin'))

  await install({}, testDefaults())

  expect(project.requireModule('@pnpm.e2e/hello-world-js-bin/package.json').isLocal).toBeTruthy()
})

test('relative link is rewritten by named installation to regular dependency', async () => {
  await addDistTag({ package: '@pnpm.e2e/hello-world-js-bin', version: '1.0.0', distTag: 'latest' })
  const project = prepareEmpty()

  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgPath = path.resolve('..', linkedPkgName)

  const opts = testDefaults({ fastUnpack: false })

  f.copy(linkedPkgName, linkedPkgPath)
  symlinkDir.sync(linkedPkgPath, path.resolve('node_modules/@pnpm.e2e/hello-world-js-bin'))

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/hello-world-js-bin'], opts)

  expect(manifest.dependencies).toStrictEqual({ '@pnpm.e2e/hello-world-js-bin': '^1.0.0' })

  expect(project.requireModule('@pnpm.e2e/hello-world-js-bin/package.json').isLocal).toBeFalsy()

  const wantedLockfile = project.readLockfile()
  expect(wantedLockfile.importers['.'].dependencies?.['@pnpm.e2e/hello-world-js-bin'].version).toBe('1.0.0')

  const currentLockfile = project.readCurrentLockfile()
  expect(currentLockfile.importers['.'].dependencies?.['@pnpm.e2e/hello-world-js-bin'].version).toBe('1.0.0')
})

// test.skip('relative link when an external lockfile is used', async () => {
//   const projects = prepare(t, [
//     {
//       name: 'project',
//       version: '1.0.0',

//       dependencies: {},
//     },
//   ])

//   const opts = testDefaults({ lockfileDir: path.join('..') })
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
