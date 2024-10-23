import path from 'path'
import { install } from '@pnpm/core'
import { fixtures } from '@pnpm/test-fixtures'
import { prepareEmpty } from '@pnpm/prepare'
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
