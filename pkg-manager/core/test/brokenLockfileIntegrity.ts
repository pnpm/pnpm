import { WANTED_LOCKFILE } from '@pnpm/constants'
import { type TarballResolution } from '@pnpm/lockfile-file'
import { prepareEmpty } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import { type ProjectRootDir } from '@pnpm/types'
import { sync as rimraf } from '@zkochan/rimraf'
import clone from 'ramda/src/clone'
import {
  addDependenciesToPackage,
  mutateModulesInSingleProject,
} from '@pnpm/core'
import { sync as writeYamlFile } from 'write-yaml-file'
import { testDefaults } from './utils'

test('installation breaks if the lockfile contains the wrong checksum', async () => {
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.0.0', distTag: 'latest' })
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({},
    [
      '@pnpm.e2e/pkg-with-1-dep@100.0.0',
    ],
    testDefaults()
  )

  rimraf('node_modules')
  const corruptedLockfile = project.readLockfile()
  const correctLockfile = clone(corruptedLockfile)
  // breaking the lockfile
  ;(corruptedLockfile.packages['@pnpm.e2e/pkg-with-1-dep@100.0.0'].resolution as TarballResolution).integrity = (corruptedLockfile.packages['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0'].resolution as TarballResolution).integrity
  writeYamlFile(WANTED_LOCKFILE, corruptedLockfile, { lineWidth: 1000 })

  await expect(mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ frozenLockfile: true }))).rejects.toThrowError(/Package name mismatch found while reading/)

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults())

  expect(project.readLockfile()).toStrictEqual(correctLockfile)

  // Breaking the lockfile again
  writeYamlFile(WANTED_LOCKFILE, corruptedLockfile, { lineWidth: 1000 })

  rimraf('node_modules')

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ preferFrozenLockfile: false }))

  expect(project.readLockfile()).toStrictEqual(correctLockfile)
})

test('installation breaks if the lockfile contains the wrong checksum and the store is clean', async () => {
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.0.0', distTag: 'latest' })
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({},
    [
      '@pnpm.e2e/pkg-with-1-dep@100.0.0',
    ],
    testDefaults({ lockfileOnly: true })
  )

  const corruptedLockfile = project.readLockfile()
  const correctIntegrity = (corruptedLockfile.packages['@pnpm.e2e/pkg-with-1-dep@100.0.0'].resolution as TarballResolution).integrity
  // breaking the lockfile
  ;(corruptedLockfile.packages['@pnpm.e2e/pkg-with-1-dep@100.0.0'].resolution as TarballResolution).integrity = 'sha512-pl8WtlGAnoIQ7gPxT187/YwhKRnsFBR4h0YY+v0FPQjT5WPuZbI9dPRaKWgKBFOqWHylJ8EyPy34V5u9YArfng=='
  writeYamlFile(WANTED_LOCKFILE, corruptedLockfile, { lineWidth: 1000 })

  await expect(
    mutateModulesInSingleProject({
      manifest,
      mutation: 'install',
      rootDir: process.cwd() as ProjectRootDir,
    }, testDefaults({ frozenLockfile: true }, { retry: { retries: 0 } }))
  ).rejects.toThrowError(/Got unexpected checksum/)

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({}, { retry: { retries: 0 } }))

  {
    const lockfile = project.readLockfile()
    expect((lockfile.packages['@pnpm.e2e/pkg-with-1-dep@100.0.0'].resolution as TarballResolution).integrity).toBe(correctIntegrity)
  }

  // Breaking the lockfile again
  writeYamlFile(WANTED_LOCKFILE, corruptedLockfile, { lineWidth: 1000 })

  rimraf('node_modules')

  const reporter = jest.fn()
  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ preferFrozenLockfile: false, reporter }, { retry: { retries: 0 } }))

  expect(reporter).toHaveBeenCalledWith(expect.objectContaining({
    level: 'warn',
    name: 'pnpm',
    prefix: process.cwd(),
    message: expect.stringMatching(/Got unexpected checksum/),
  }))
  {
    const lockfile = project.readLockfile()
    expect((lockfile.packages['@pnpm.e2e/pkg-with-1-dep@100.0.0'].resolution as TarballResolution).integrity).toBe(correctIntegrity)
  }
})
