import fs from 'fs'
import path from 'path'
import { install } from '@pnpm/core'
import { readWantedLockfile } from '@pnpm/lockfile.fs'
import { preparePackages } from '@pnpm/prepare'
import { testDefaults } from '../utils/index.js'

test('throws an error when the peerDependencies have unallowed specs', async () => {
  preparePackages([
    {
      name: 'foo',
      version: '1.0.0',
      private: true,
    },
  ])

  // eslint-disable-next-line
  const { rejects } = expect(
    install({
      name: 'root',
      version: '0.0.0',
      private: true,
      peerDependencies: {
        foo: 'link:foo',
      },
    }, testDefaults())
  )

  await rejects.toHaveProperty(['code'], 'ERR_PNPM_INVALID_PEER_DEPENDENCY_SPECIFICATION')
  await rejects.toHaveProperty(['message'], "The peerDependencies field named 'foo' of package 'root' has an invalid value: 'link:foo'")
})

test('overrides are not prevented from replacing peerDependencies with local links', async () => {
  preparePackages([
    {
      name: 'fake-is-positive',
      version: '1.0.0',
      private: true,
    },
  ])

  const overrides = {
    'is-positive': 'link:fake-is-positive',
  }

  await install({
    name: 'root',
    version: '0.0.0',
    private: true,
    dependencies: {
      'is-positive': '1.0.0',
    },
    peerDependencies: {
      'is-positive': '^1.0.0',
    },
  }, testDefaults({ overrides }))

  expect(await readWantedLockfile('.', { ignoreIncompatible: false })).toMatchObject({
    overrides,
    importers: {
      '.': {
        dependencies: {
          'is-positive': overrides['is-positive'],
        },
        specifiers: {
          'is-positive': overrides['is-positive'],
        },
      },
    },
  })

  expect(fs.realpathSync('node_modules/is-positive')).toBe(path.resolve('fake-is-positive'))
})

test("empty overrides don't disable peer dependencies validation", async () => {
  preparePackages([
    {
      name: 'foo',
      version: '1.0.0',
      private: true,
    },
  ])

  const overrides = {}

  // eslint-disable-next-line
  const { rejects } = expect(
    install({
      name: 'root',
      version: '0.0.0',
      private: true,
      peerDependencies: {
        foo: 'link:foo',
      },
    }, testDefaults({ overrides }))
  )

  await rejects.toHaveProperty(['code'], 'ERR_PNPM_INVALID_PEER_DEPENDENCY_SPECIFICATION')
  await rejects.toHaveProperty(['message'], "The peerDependencies field named 'foo' of package 'root' has an invalid value: 'link:foo'")
})
