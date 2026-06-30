import { expect, test } from '@jest/globals'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import {
  addDependenciesToPackage,
  mutateModulesInSingleProject,
} from '@pnpm/installing.deps-installer'
import type { TarballResolution } from '@pnpm/lockfile.fs'
import { prepareEmpty } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/testing.registry-mock'
import type { ProjectRootDir } from '@pnpm/types'
import { rimrafSync } from '@zkochan/rimraf'
import { clone } from 'ramda'
import { writeYamlFileSync } from 'write-yaml-file'

import { testDefaults } from './utils/index.js'

test('installation fails by default if the lockfile contains a wrong checksum, but --update-checksums recovers', async () => {
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.0.0', distTag: 'latest' })
  const project = prepareEmpty()

  const { updatedManifest: manifest } = await addDependenciesToPackage({},
    [
      '@pnpm.e2e/pkg-with-1-dep@100.0.0',
    ],
    testDefaults()
  )

  rimrafSync('node_modules')
  const corruptedLockfile = project.readLockfile()
  const correctLockfile = clone(corruptedLockfile)
  // breaking the lockfile
  ;(corruptedLockfile.packages['@pnpm.e2e/pkg-with-1-dep@100.0.0'].resolution as TarballResolution).integrity = (corruptedLockfile.packages['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0'].resolution as TarballResolution).integrity
  writeYamlFileSync(WANTED_LOCKFILE, corruptedLockfile, { lineWidth: 1000 })

  await expect(mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ frozenLockfile: true }, { retry: { retries: 0 } }))).rejects.toThrow(/Got unexpected checksum for/)

  await expect(mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({}, { retry: { retries: 0 } }))).rejects.toThrow(/Got unexpected checksum for/)

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ updateChecksums: true }, { retry: { retries: 0 } }))

  expect(project.readLockfile()).toStrictEqual(correctLockfile)

  // Breaking the lockfile again
  writeYamlFileSync(WANTED_LOCKFILE, corruptedLockfile, { lineWidth: 1000 })

  rimrafSync('node_modules')

  // --force is NOT an opt-in: it should still fail.
  await expect(mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ force: true }, { retry: { retries: 0 } }))).rejects.toThrow(/Got unexpected checksum for/)
})

test('an install fails closed when a registry tarball entry in the lockfile is missing its integrity', async () => {
  const project = prepareEmpty()

  const { updatedManifest: manifest } = await addDependenciesToPackage({},
    ['is-positive@1.0.0'],
    testDefaults()
  )

  // Simulate a lockfile written by an older pnpm where the registry never
  // provided an integrity. A missing integrity is indistinguishable from a
  // tampered lockfile, so it must never be silently healed: both frozen and
  // non-frozen installs fail closed.
  const lockfileWithoutIntegrity = clone(project.readLockfile())
  delete (lockfileWithoutIntegrity.packages['is-positive@1.0.0'].resolution as TarballResolution).integrity

  writeYamlFileSync(WANTED_LOCKFILE, lockfileWithoutIntegrity, { lineWidth: 1000 })
  rimrafSync('node_modules')
  await expect(mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ frozenLockfile: true }, { retry: { retries: 0 } }))).rejects.toThrow(/has no "integrity" field/)

  writeYamlFileSync(WANTED_LOCKFILE, lockfileWithoutIntegrity, { lineWidth: 1000 })
  rimrafSync('node_modules')
  await expect(mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({}, { retry: { retries: 0 } }))).rejects.toThrow(/has no "integrity" field/)
})

test('installation fails by default if the lockfile contains the wrong checksum and the store is clean', async () => {
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.0.0', distTag: 'latest' })
  const project = prepareEmpty()

  const { updatedManifest: manifest } = await addDependenciesToPackage({},
    [
      '@pnpm.e2e/pkg-with-1-dep@100.0.0',
    ],
    testDefaults({ lockfileOnly: true })
  )

  const corruptedLockfile = project.readLockfile()
  const correctIntegrity = (corruptedLockfile.packages['@pnpm.e2e/pkg-with-1-dep@100.0.0'].resolution as TarballResolution).integrity
  // breaking the lockfile
  ;(corruptedLockfile.packages['@pnpm.e2e/pkg-with-1-dep@100.0.0'].resolution as TarballResolution).integrity = 'sha512-pl8WtlGAnoIQ7gPxT187/YwhKRnsFBR4h0YY+v0FPQjT5WPuZbI9dPRaKWgKBFOqWHylJ8EyPy34V5u9YArfng=='
  writeYamlFileSync(WANTED_LOCKFILE, corruptedLockfile, { lineWidth: 1000 })

  await expect(
    mutateModulesInSingleProject({
      manifest,
      mutation: 'install',
      rootDir: process.cwd() as ProjectRootDir,
    }, testDefaults({ frozenLockfile: true }, { retry: { retries: 0 } }))
  ).rejects.toThrow(/Got unexpected checksum/)

  await expect(mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({}, { retry: { retries: 0 } }))).rejects.toThrow(/Got unexpected checksum/)

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ updateChecksums: true }, { retry: { retries: 0 } }))

  {
    const lockfile = project.readLockfile()
    expect((lockfile.packages['@pnpm.e2e/pkg-with-1-dep@100.0.0'].resolution as TarballResolution).integrity).toBe(correctIntegrity)
  }
})
