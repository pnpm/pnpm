import path from 'path'
import { prepareEmpty } from '@pnpm/prepare'
import { addUser, REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { addDependenciesToPackage, install } from '@pnpm/core'
import rimraf from '@zkochan/rimraf'
import { testDefaults } from '../utils'

const skipOnNode17 = process.version.split('.')[0] === 'v17' ? test.skip : test

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
  const manifest = await addDependenciesToPackage({}, ['needs-auth'], await testDefaults({}, {
    authConfig,
  }, {
    authConfig,
  }))

  await project.has('needs-auth')

  // should work when a lockfile is available
  // and the registry in .npmrc is not the same as the one in lockfile
  await rimraf('node_modules')
  await rimraf(path.join('..', '.store'))

  authConfig = {
    [`//localhost:${REGISTRY_MOCK_PORT}/:_authToken`]: data.token,
    registry: 'https://registry.npmjs.org/',
  }
  await addDependenciesToPackage(manifest, ['needs-auth'], await testDefaults({}, {
    authConfig,
    registry: 'https://registry.npmjs.org/',
  }, {
    authConfig,
  }))

  await project.has('needs-auth')
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
  await addDependenciesToPackage({}, ['needs-auth'], await testDefaults({}, {
    authConfig,
  }, {
    authConfig,
  }))

  await project.has('needs-auth')
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
    'always-auth': true,
    registry: `http://localhost:${REGISTRY_MOCK_PORT}`,
  }
  await addDependenciesToPackage({}, ['needs-auth'], await testDefaults({}, {
    authConfig,
  }, {
    authConfig,
  }))

  await project.has('needs-auth')
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

  await project.has('@private/foo')

  // should work when a lockfile is available
  await rimraf('node_modules')
  await rimraf(path.join('..', '.store'))

  // Recreating options to have a new storeController with clean cache
  opts = await testDefaults({}, {
    authConfig,
    registry: 'https://registry.npmjs.org/',
  }, {
    authConfig,
  })
  await addDependenciesToPackage(manifest, ['@private/foo'], opts)

  await project.has('@private/foo')
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
    [`//localhost:${REGISTRY_MOCK_PORT}/:always-auth`]: true,
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

  await project.has('@private/foo')

  // should work when a lockfile is available
  await rimraf('node_modules')
  await rimraf(path.join('..', '.store'))

  // Recreating options to have a new storeController with clean cache
  opts = await testDefaults({}, {
    authConfig,
    registry: 'https://registry.npmjs.org/',
  }, {
    authConfig,
  })
  await addDependenciesToPackage(manifest, ['@private/foo'], opts)

  await project.has('@private/foo')
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
    [`//127.0.0.1:${REGISTRY_MOCK_PORT}/:always-auth`]: true,
    registry: `http://127.0.0.1:${REGISTRY_MOCK_PORT}`,
  }
  await addDependenciesToPackage({}, ['needs-auth'], await testDefaults({
    registries: {
      default: `http://127.0.0.1:${REGISTRY_MOCK_PORT}`,
    },
  }, {
    authConfig,
    registry: `http://127.0.0.1:${REGISTRY_MOCK_PORT}`,
  }, {
    authConfig,
  }))

  await project.has('needs-auth')
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
    [`//127.0.0.1:${REGISTRY_MOCK_PORT}/:always-auth`]: true,
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

  const manifest = await addDependenciesToPackage({}, ['needs-auth'], opts)

  await rimraf('node_modules')
  await rimraf(path.join('..', '.registry'))
  await rimraf(path.join('..', '.store'))

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

  await project.has('needs-auth')
})
