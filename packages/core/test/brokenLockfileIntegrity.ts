import { WANTED_LOCKFILE } from '@pnpm/constants'
import { prepareEmpty } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import rimraf from '@zkochan/rimraf'
import clone from 'ramda/src/clone'
import {
  addDependenciesToPackage,
  mutateModulesInSingleProject,
} from '@pnpm/core'
import writeYamlFile from 'write-yaml-file'
import { testDefaults } from './utils'

test('installation breaks if the lockfile contains the wrong checksum', async () => {
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.0.0', distTag: 'latest' })
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({},
    [
      '@pnpm.e2e/pkg-with-1-dep@100.0.0',
    ],
    await testDefaults({ lockfileOnly: true })
  )

  const corruptedLockfile = await project.readLockfile()
  const correctLockfile = clone(corruptedLockfile)
  // breaking the lockfile
  corruptedLockfile.packages['/@pnpm.e2e/pkg-with-1-dep/100.0.0'].resolution['integrity'] = corruptedLockfile.packages['/@pnpm.e2e/dep-of-pkg-with-1-dep/100.0.0'].resolution['integrity']
  await writeYamlFile(WANTED_LOCKFILE, corruptedLockfile, { lineWidth: 1000 })

  await expect(mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd(),
  }, await testDefaults({ frozenLockfile: true }))).rejects.toThrowError(/Package name mismatch found while reading/)

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd(),
  }, await testDefaults())

  expect(await project.readLockfile()).toStrictEqual(correctLockfile)

  // Breaking the lockfile again
  await writeYamlFile(WANTED_LOCKFILE, corruptedLockfile, { lineWidth: 1000 })

  await rimraf('node_modules')

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd(),
  }, await testDefaults({ preferFrozenLockfile: false }))

  expect(await project.readLockfile()).toStrictEqual(correctLockfile)
})

test('installation breaks if the lockfile contains the wrong checksum and the store is clean', async () => {
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.0.0', distTag: 'latest' })
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({},
    [
      '@pnpm.e2e/pkg-with-1-dep@100.0.0',
    ],
    await testDefaults({ lockfileOnly: true })
  )

  const corruptedLockfile = await project.readLockfile()
  const correctIntegrity = corruptedLockfile.packages['/@pnpm.e2e/pkg-with-1-dep/100.0.0'].resolution['integrity']
  // breaking the lockfile
  corruptedLockfile.packages['/@pnpm.e2e/pkg-with-1-dep/100.0.0'].resolution['integrity'] = 'sha512-pl8WtlGAnoIQ7gPxT187/YwhKRnsFBR4h0YY+v0FPQjT5WPuZbI9dPRaKWgKBFOqWHylJ8EyPy34V5u9YArfng=='
  await writeYamlFile(WANTED_LOCKFILE, corruptedLockfile, { lineWidth: 1000 })

  await expect(
    mutateModulesInSingleProject({
      manifest,
      mutation: 'install',
      rootDir: process.cwd(),
    }, await testDefaults({ frozenLockfile: true }, { retry: { retries: 0 } }))
  ).rejects.toThrowError(/Got unexpected checksum/)

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd(),
  }, await testDefaults({}, { retry: { retries: 0 } }))

  {
    const lockfile = await project.readLockfile()
    expect(lockfile.packages['/@pnpm.e2e/pkg-with-1-dep/100.0.0'].resolution['integrity']).toBe(correctIntegrity)
  }

  // Breaking the lockfile again
  await writeYamlFile(WANTED_LOCKFILE, corruptedLockfile, { lineWidth: 1000 })

  await rimraf('node_modules')

  const reporter = jest.fn()
  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd(),
  }, await testDefaults({ preferFrozenLockfile: false, reporter }, { retry: { retries: 0 } }))

  expect(reporter).toBeCalledWith(expect.objectContaining({
    level: 'warn',
    name: 'pnpm',
    prefix: process.cwd(),
    message: expect.stringMatching(/Got unexpected checksum/),
  }))
  {
    const lockfile = await project.readLockfile()
    expect(lockfile.packages['/@pnpm.e2e/pkg-with-1-dep/100.0.0'].resolution['integrity']).toBe(correctIntegrity)
  }
})
