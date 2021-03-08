import { WANTED_LOCKFILE } from '@pnpm/constants'
import { prepareEmpty } from '@pnpm/prepare'
import rimraf from '@zkochan/rimraf'
import path from 'path'
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

test('installation breaks if the lockfile contains the wrong checksum and the store is clean', async () => {
  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', 'latest')
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({},
    [
      'pkg-with-1-dep@100.0.0',
    ],
    await testDefaults({ lockfileOnly: true })
  )

  const corruptedLockfile = await project.readLockfile()
  const correctIntegrity = corruptedLockfile.packages['/pkg-with-1-dep/100.0.0'].resolution['integrity']
  // breaking the lockfile
  corruptedLockfile.packages['/pkg-with-1-dep/100.0.0'].resolution['integrity'] = 'sha512-pl8WtlGAnoIQ7gPxT187/YwhKRnsFBR4h0YY+v0FPQjT5WPuZbI9dPRaKWgKBFOqWHylJ8EyPy34V5u9YArfng=='
  await writeYamlFile(WANTED_LOCKFILE, corruptedLockfile, { lineWidth: 1000 })

  const storeDir = path.resolve('new-store')
  const opts = await testDefaults({
    storeDir,
  }, {
    retry: { retries: 0 },
  })
  await expect(mutateModules([
    {
      buildIndex: 0,
      manifest,
      mutation: 'install',
      rootDir: process.cwd(),
    },
  ], { ...opts, frozenLockfile: true })).rejects.toThrowError(/Got unexpected checksum/)

  await mutateModules([
    {
      buildIndex: 0,
      manifest,
      mutation: 'install',
      rootDir: process.cwd(),
    },
  ], opts)

  {
    const lockfile = await project.readLockfile()
    expect(lockfile.packages['/pkg-with-1-dep/100.0.0'].resolution['integrity']).toBe(correctIntegrity)
  }

  // Breaking the lockfile again
  await writeYamlFile(WANTED_LOCKFILE, corruptedLockfile, { lineWidth: 1000 })

  await rimraf('node_modules')

  const reporter = jest.fn()
  await mutateModules([
    {
      buildIndex: 0,
      manifest,
      mutation: 'install',
      rootDir: process.cwd(),
    },
  ], await testDefaults({ preferFrozenLockfile: false, reporter, storeDir }, { retry: { retries: 0 } }))

  expect(reporter).toBeCalledWith(expect.objectContaining({
    level: 'warn',
    name: 'pnpm',
    prefix: process.cwd(),
    message: 'The lockfile is broken! A full installation will be performed in an attempt to fix it.',
  }))
  {
    const lockfile = await project.readLockfile()
    expect(lockfile.packages['/pkg-with-1-dep/100.0.0'].resolution['integrity']).toBe(correctIntegrity)
  }
})
