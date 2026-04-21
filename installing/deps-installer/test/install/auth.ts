import path from 'node:path'

import { test } from '@jest/globals'
import { addDependenciesToPackage, install } from '@pnpm/installing.deps-installer'
import { prepareEmpty } from '@pnpm/prepare'
import { addUser, REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import type { RegistryConfig } from '@pnpm/types'
import { rimrafSync } from '@zkochan/rimraf'

import { testDefaults } from '../utils/index.js'

const skipOnNode17 = ['v14', 'v16'].includes(process.version.split('.')[0]) ? test : test.skip

test('a package that need authentication', async () => {
  const project = prepareEmpty()

  const data = await addUser({
    email: 'foo@bar.com',
    password: 'bar',
    username: 'foo',
  })

  let configByUri: Record<string, RegistryConfig> = {
    [`//localhost:${REGISTRY_MOCK_PORT}/`]: { creds: { authToken: data.token } },
  }
  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['@pnpm.e2e/needs-auth'], testDefaults({}, {
    configByUri,
  }, {
    configByUri,
  }))

  project.has('@pnpm.e2e/needs-auth')

  // should work when a lockfile is available
  // and the registry in .npmrc is not the same as the one in lockfile
  rimrafSync('node_modules')
  rimrafSync(path.join('..', '.store'))

  configByUri = {
    [`//localhost:${REGISTRY_MOCK_PORT}/`]: { creds: { authToken: data.token } },
  }
  await addDependenciesToPackage(manifest, ['@pnpm.e2e/needs-auth'], testDefaults({}, {
    configByUri,
    registry: 'https://registry.npmjs.org/',
  }, {
    configByUri,
  }))

  project.has('@pnpm.e2e/needs-auth')
})

test('installing a package that need authentication, using password', async () => {
  const project = prepareEmpty()

  await addUser({
    email: 'foo@bar.com',
    password: 'bar',
    username: 'foo',
  })

  const configByUri: Record<string, RegistryConfig> = {
    [`//localhost:${REGISTRY_MOCK_PORT}/`]: { creds: { basicAuth: { username: 'foo', password: 'bar' } } },
  }
  await addDependenciesToPackage({}, ['@pnpm.e2e/needs-auth'], testDefaults({}, {
    configByUri,
  }, {
    configByUri,
  }))

  project.has('@pnpm.e2e/needs-auth')
})

test('a package that need authentication, legacy way', async () => {
  const project = prepareEmpty()

  await addUser({
    email: 'foo@bar.com',
    password: 'bar',
    username: 'foo',
  })

  const configByUri: Record<string, RegistryConfig> = {
    '': { creds: { basicAuth: { username: 'foo', password: 'bar' } } },
  }
  await addDependenciesToPackage({}, ['@pnpm.e2e/needs-auth'], testDefaults({}, {
    configByUri,
  }, {
    configByUri,
  }))

  project.has('@pnpm.e2e/needs-auth')
})

test('a scoped package that need authentication specific to scope', async () => {
  const project = prepareEmpty()

  const data = await addUser({
    email: 'foo@bar.com',
    password: 'bar',
    username: 'foo',
  })

  const configByUri: Record<string, RegistryConfig> = {
    [`//localhost:${REGISTRY_MOCK_PORT}/`]: { creds: { authToken: data.token } },
  }
  let opts = testDefaults({
    registries: {
      default: 'https://registry.npmjs.org/',
      '@private': `http://localhost:${REGISTRY_MOCK_PORT}/`,
    },
  }, {
    configByUri,
    registry: 'https://registry.npmjs.org/',
  }, {
    configByUri,
  })
  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['@private/foo'], opts)

  project.has('@private/foo')

  // should work when a lockfile is available
  rimrafSync('node_modules')
  rimrafSync(path.join('..', '.store'))

  // Recreating options to have a new storeController with clean cache
  opts = testDefaults({
    registries: {
      default: 'https://registry.npmjs.org/',
      '@private': `http://localhost:${REGISTRY_MOCK_PORT}/`,
    },
  }, {
    configByUri,
    registry: 'https://registry.npmjs.org/',
  }, {
    configByUri,
  })
  await addDependenciesToPackage(manifest, ['@private/foo'], opts)

  project.has('@private/foo')
})

test('a scoped package that need legacy authentication specific to scope', async () => {
  const project = prepareEmpty()

  await addUser({
    email: 'foo@bar.com',
    password: 'bar',
    username: 'foo',
  })

  const configByUri: Record<string, RegistryConfig> = {
    [`//localhost:${REGISTRY_MOCK_PORT}/`]: { creds: { basicAuth: { username: 'foo', password: 'bar' } } },
  }
  let opts = testDefaults({
    registries: {
      default: 'https://registry.npmjs.org/',
      '@private': `http://localhost:${REGISTRY_MOCK_PORT}/`,
    },
  }, {
    configByUri,
    registry: 'https://registry.npmjs.org/',
  }, {
    configByUri,
  })
  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['@private/foo'], opts)

  project.has('@private/foo')

  // should work when a lockfile is available
  rimrafSync('node_modules')
  rimrafSync(path.join('..', '.store'))

  // Recreating options to have a new storeController with clean cache
  opts = testDefaults({
    registries: {
      default: 'https://registry.npmjs.org/',
      '@private': `http://localhost:${REGISTRY_MOCK_PORT}/`,
    },
  }, {
    configByUri,
    registry: 'https://registry.npmjs.org/',
  }, {
    configByUri,
  })
  await addDependenciesToPackage(manifest, ['@private/foo'], opts)

  project.has('@private/foo')
})

skipOnNode17('a package that need authentication reuses authorization tokens for tarball fetching', async () => {
  const project = prepareEmpty()

  const data = await addUser({
    email: 'foo@bar.com',
    password: 'bar',
    username: 'foo',
  })

  const configByUri: Record<string, RegistryConfig> = {
    [`//127.0.0.1:${REGISTRY_MOCK_PORT}/`]: { creds: { authToken: data.token } },
  }
  await addDependenciesToPackage({}, ['@pnpm.e2e/needs-auth'], testDefaults({
    registries: {
      default: `http://127.0.0.1:${REGISTRY_MOCK_PORT}`,
    },
  }, {
    configByUri,
    registry: `http://127.0.0.1:${REGISTRY_MOCK_PORT}`,
  }, {
    configByUri,
  }))

  project.has('@pnpm.e2e/needs-auth')
})

skipOnNode17('a package that need authentication reuses authorization tokens for tarball fetching when meta info is cached', async () => {
  const project = prepareEmpty()

  const data = await addUser({
    email: 'foo@bar.com',
    password: 'bar',
    username: 'foo',
  })

  const configByUri: Record<string, RegistryConfig> = {
    [`//127.0.0.1:${REGISTRY_MOCK_PORT}/`]: { creds: { authToken: data.token } },
  }
  let opts = testDefaults({
    registries: {
      default: `http://127.0.0.1:${REGISTRY_MOCK_PORT}`,
    },
  }, {
    configByUri,
    registry: `http://127.0.0.1:${REGISTRY_MOCK_PORT}`,
  }, {
    configByUri,
  })

  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['@pnpm.e2e/needs-auth'], opts)

  rimrafSync('node_modules')
  rimrafSync(path.join('..', '.registry'))
  rimrafSync(path.join('..', '.store'))

  // Recreating options to clean store cache
  opts = testDefaults({
    registries: {
      default: `http://127.0.0.1:${REGISTRY_MOCK_PORT}`,
    },
  }, {
    configByUri,
    registry: `http://127.0.0.1:${REGISTRY_MOCK_PORT}`,
  }, {
    configByUri,
  })
  await install(manifest, opts)

  project.has('@pnpm.e2e/needs-auth')
})
