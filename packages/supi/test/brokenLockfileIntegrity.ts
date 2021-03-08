import { WANTED_LOCKFILE } from '@pnpm/constants'
import { prepareEmpty } from '@pnpm/prepare'
import rimraf from '@zkochan/rimraf'
import {
  addDependenciesToPackage,
  mutateModules,
} from 'supi'
import writeYamlFile from 'write-yaml-file'
import {
  addDistTag,
  testDefaults,
} from './utils'

test('installation breaks if the lockfile contains the wrong checksum', async () => {
  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', 'latest')
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({},
    [
      'pkg-with-1-dep@100.0.0',
    ],
    await testDefaults({ lockfileOnly: true })
  )

  const lockfile = await project.readLockfile()
  // breaking the lockfile
  lockfile.packages['/pkg-with-1-dep/100.0.0'].resolution['integrity'] = lockfile.packages['/dep-of-pkg-with-1-dep/100.0.0'].resolution['integrity']
  await writeYamlFile(WANTED_LOCKFILE, lockfile, { lineWidth: 1000 })

  await expect(mutateModules([
    {
      buildIndex: 0,
      manifest,
      mutation: 'install',
      rootDir: process.cwd(),
    },
  ], await testDefaults({ frozenLockfile: true }))).rejects.toThrowError(/Package name mismatch found while reading/)

  await mutateModules([
    {
      buildIndex: 0,
      manifest,
      mutation: 'install',
      rootDir: process.cwd(),
    },
  ], await testDefaults())

  // Breaking the lockfile again
  await writeYamlFile(WANTED_LOCKFILE, lockfile, { lineWidth: 1000 })

  await rimraf('node_modules')

  await mutateModules([
    {
      buildIndex: 0,
      manifest,
      mutation: 'install',
      rootDir: process.cwd(),
    },
  ], await testDefaults({ preferFrozenLockfile: false }))
})
