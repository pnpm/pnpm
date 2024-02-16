import path from 'path'
import { prepareEmpty } from '@pnpm/prepare'
import { addUser, REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { addDependenciesToPackage, install } from '@pnpm/core'
import { sync as rimraf } from '@zkochan/rimraf'
import { testDefaults } from '../utils'

const skipOnNode17 = ['v14', 'v16'].includes(process.version.split('.')[0]) ? test : test.skip

test('a package that need authentication', async () => {
  const project = prepareEmpty()

  const data = await addUser({
    email: 'foo@bar.com',
    password: 'bar',
    username: 'foo',
  })

  let authConfig = {
    [`//localhost:${REGISTRY_MOCK_PORT}/:_authToken`]: data.token,
    registry: `http://localhost:${REGISTRY_MOCK_PORT}/`,
  }
  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/needs-auth'], await testDefaults({}, {
    authConfig,
  }, {
    authConfig,
  }))

  project.has('@pnpm.e2e/needs-auth')

  // should work when a lockfile is available
  // and the registry in .npmrc is not the same as the one in lockfile
  rimraf('node_modules')
  rimraf(path.join('..', '.store'))

  authConfig = {
    [`//localhost:${REGISTRY_MOCK_PORT}/:_authToken`]: data.token,
    registry: 'https://registry.npmjs.org/',
  }
  await addDependenciesToPackage(manifest, ['@pnpm.e2e/needs-auth'], await testDefaults({}, {
    authConfig,
    registry: 'https://registry.npmjs.org/',
  }, {
    authConfig,
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

  const encodedPassword = Buffer.from('bar').toString('base64')
  const authConfig = {
    [`//localhost:${REGISTRY_MOCK_PORT}/:_password`]: encodedPassword,
    [`//localhost:${REGISTRY_MOCK_PORT}/:username`]: 'foo',
    registry: `http://localhost:${REGISTRY_MOCK_PORT}/`,
  }
  await addDependenciesToPackage({}, ['@pnpm.e2e/needs-auth'], await testDefaults({}, {
    authConfig,
  }, {
    authConfig,
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

  const authConfig = {
    _auth: 'Zm9vOmJhcg==', // base64 encoded foo:bar
    registry: `http://localhost:${REGISTRY_MOCK_PORT}`,
  }
  await addDependenciesToPackage({}, ['@pnpm.e2e/needs-auth'], await testDefaults({}, {
    authConfig,
  }, {
    authConfig,
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

  const authConfig = {
    [`//localhost:${REGISTRY_MOCK_PORT}/:_authToken`]: data.token,
    '@private:registry': `http://localhost:${REGISTRY_MOCK_PORT}/`,
    registry: 'https://registry.npmjs.org/',
  }
  let opts = await testDefaults({}, {
    authConfig,
    registry: 'https://registry.npmjs.org/',
  }, {
    authConfig,
  })
  const manifest = await addDependenciesToPackage({}, ['@private/foo'], opts)

  project.has('@private/foo')

  // should work when a lockfile is available
  rimraf('node_modules')
  rimraf(path.join('..', '.store'))

  // Recreating options to have a new storeController with clean cache
  opts = await testDefaults({}, {
    authConfig,
    registry: 'https://registry.npmjs.org/',
  }, {
    authConfig,
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

  const authConfig = {
    [`//localhost:${REGISTRY_MOCK_PORT}/:_auth`]: 'Zm9vOmJhcg==', // base64 encoded foo:bar
    '@private:registry': `http://localhost:${REGISTRY_MOCK_PORT}/`,
    registry: 'https://registry.npmjs.org/',
  }
  let opts = await testDefaults({}, {
    authConfig,
    registry: 'https://registry.npmjs.org/',
  }, {
    authConfig,
  })
  const manifest = await addDependenciesToPackage({}, ['@private/foo'], opts)

  project.has('@private/foo')

  // should work when a lockfile is available
  rimraf('node_modules')
  rimraf(path.join('..', '.store'))

  // Recreating options to have a new storeController with clean cache
  opts = await testDefaults({}, {
    authConfig,
    registry: 'https://registry.npmjs.org/',
  }, {
    authConfig,
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

  const authConfig = {
    [`//127.0.0.1:${REGISTRY_MOCK_PORT}/:_authToken`]: data.token,
    registry: `http://127.0.0.1:${REGISTRY_MOCK_PORT}`,
  }
  await addDependenciesToPackage({}, ['@pnpm.e2e/needs-auth'], await testDefaults({
    registries: {
      default: `http://127.0.0.1:${REGISTRY_MOCK_PORT}`,
    },
  }, {
    authConfig,
    registry: `http://127.0.0.1:${REGISTRY_MOCK_PORT}`,
  }, {
    authConfig,
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

  const authConfig = {
    [`//127.0.0.1:${REGISTRY_MOCK_PORT}/:_authToken`]: data.token,
    registry: `http://127.0.0.1:${REGISTRY_MOCK_PORT}`,
  }
  let opts = await testDefaults({
    registries: {
      default: `http://127.0.0.1:${REGISTRY_MOCK_PORT}`,
    },
  }, {
    authConfig,
    registry: `http://127.0.0.1:${REGISTRY_MOCK_PORT}`,
  }, {
    authConfig,
  })

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/needs-auth'], opts)

  rimraf('node_modules')
  rimraf(path.join('..', '.registry'))
  rimraf(path.join('..', '.store'))

  // Recreating options to clean store cache
  opts = await testDefaults({
    registries: {
      default: `http://127.0.0.1:${REGISTRY_MOCK_PORT}`,
    },
  }, {
    authConfig,
    registry: `http://127.0.0.1:${REGISTRY_MOCK_PORT}`,
  }, {
    authConfig,
  })
  await install(manifest, opts)

  project.has('@pnpm.e2e/needs-auth')
})
