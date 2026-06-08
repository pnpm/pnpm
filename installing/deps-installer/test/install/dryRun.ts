import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import {
  addDependenciesToPackage,
  install,
} from '@pnpm/installing.deps-installer'
import { prepareEmpty } from '@pnpm/prepare'

import { testDefaults } from '../utils/index.js'

test('dry run succeeds when lockfile is up-to-date', async () => {
  prepareEmpty()

  await addDependenciesToPackage({}, ['is-positive@1.0.0'], testDefaults())

  // dry run with same manifest should succeed without throwing
  await install({
    dependencies: {
      'is-positive': '1.0.0',
    },
  }, testDefaults({
    frozenLockfile: true,
    lockfileOnly: true,
  }))
})

test('dry run fails when lockfile is not up-to-date with package.json', async () => {
  prepareEmpty()

  await addDependenciesToPackage({}, ['is-positive@1.0.0'], testDefaults())

  // dry run with a changed dep should throw
  await expect(
    install({
      dependencies: {
        'is-positive': '^3.1.0',
      },
    }, testDefaults({
      frozenLockfile: true,
      lockfileOnly: true,
    }))
  ).rejects.toThrow(`Cannot install with "frozen-lockfile" because ${WANTED_LOCKFILE} is not up to date with ${path.join('<ROOT>', 'package.json')}`)
})

test('dry run does not write a lockfile on a fresh project', async () => {
  prepareEmpty()

  // frozenLockfile throws before any write because no lockfile exists yet
  await expect(
    install({
      dependencies: {
        'is-positive': '1.0.0',
      },
    }, testDefaults({
      frozenLockfile: true,
      lockfileOnly: true,
    }))
  ).rejects.toThrow()

  expect(fs.existsSync(WANTED_LOCKFILE)).toBeFalsy()
})

test('dry run does not modify existing lockfile', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['is-positive@1.0.0'], testDefaults())

  // Attempt a dry run that would fail (stale lockfile)
  await expect(
    install({
      dependencies: {
        'is-positive': '2.0.0',
      },
    }, testDefaults({
      frozenLockfile: true,
      lockfileOnly: true,
    }))
  ).rejects.toThrow()

  // Lockfile should still reference the original version
  const lockfile = await project.readLockfile()
  expect(lockfile.packages['is-positive@1.0.0']).toBeTruthy()
  expect(lockfile.packages['is-positive@2.0.0']).toBeFalsy()
})

test('dry run does not install packages to node_modules', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['is-positive@1.0.0'], testDefaults())

  // Remove node_modules to prove dry run doesn't recreate it
  fs.rmSync('node_modules', { recursive: true, force: true })

  await install({
    dependencies: {
      'is-positive': '1.0.0',
    },
  }, testDefaults({
    frozenLockfile: true,
    lockfileOnly: true,
  }))

  project.hasNot('is-positive')
})
