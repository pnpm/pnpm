import { prepareEmpty } from '@pnpm/prepare'
import rimraf = require('@zkochan/rimraf')
import RegClient = require('anonymous-npm-registry-client')
import path = require('path')
import { addDependenciesToPackage, install } from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { testDefaults } from '../utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('a package that need authentication', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  const client = new RegClient()

  const data = await new Promise((resolve, reject) => {
    client.adduser('http://localhost:4873', {
      auth: {
        email: 'foo@bar.com',
        password: 'bar',
        username: 'foo',
      },
    }, (err: Error, d: { token: string }) => err ? reject(err) : resolve(d))
  }) as {token: string}

  let rawConfig = {
    '//localhost:4873/:_authToken': data.token,
    'registry': 'http://localhost:4873/',
  }
  const manifest = await addDependenciesToPackage({}, ['needs-auth'], await testDefaults({}, {
    rawConfig,
  }, {
    rawConfig,
  }))

  const m = project.requireModule('needs-auth')

  t.ok(typeof m === 'function', 'needs-auth() is available')

  // should work when a lockfile is available
  // and the registry in .npmrc is not the same as the one in lockfile
  await rimraf('node_modules')
  await rimraf(path.join('..', '.store'))

  rawConfig = {
    '//localhost:4873/:_authToken': data.token,
    'registry': 'https://registry.npmjs.org/',
  }
  await addDependenciesToPackage(manifest, ['needs-auth'], await testDefaults({}, {
    rawConfig,
    registry: 'https://registry.npmjs.org/',
  }, {
    rawConfig,
  }))

  await project.has('needs-auth')
})

test('installing a package that need authentication, using password', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  const client = new RegClient()

  const data = await new Promise((resolve, reject) => {
    client.adduser('http://localhost:4873', {
      auth: {
        email: 'foo@bar.com',
        password: 'bar',
        username: 'foo',
      },
    }, (err: Error, d: { token: string }) => err ? reject(err) : resolve(d))
  }) as {token: string}

  const encodedPassword = Buffer.from('bar').toString('base64')
  let rawConfig = {
    '//localhost:4873/:_password': encodedPassword,
    '//localhost:4873/:username': 'foo',
    'registry': 'http://localhost:4873/',
  }
  await addDependenciesToPackage({}, ['needs-auth'], await testDefaults({}, {
    rawConfig,
  }, {
    rawConfig,
  }))

  const m = project.requireModule('needs-auth')

  t.ok(typeof m === 'function', 'needs-auth() is available')
})

test('a package that need authentication, legacy way', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  const client = new RegClient()

  const data = await new Promise((resolve, reject) => {
    client.adduser('http://localhost:4873', {
      auth: {
        email: 'foo@bar.com',
        password: 'bar',
        username: 'foo',
      },
    }, (err: Error, d: object) => err ? reject(err) : resolve(d))
  })

  const rawConfig = {
    '_auth': 'Zm9vOmJhcg==', // base64 encoded foo:bar
    'always-auth': true,
    'registry': 'http://localhost:4873',
  }
  await addDependenciesToPackage({}, ['needs-auth'], await testDefaults({}, {
    rawConfig,
  }, {
    rawConfig,
  }))

  const m = project.requireModule('needs-auth')

  t.ok(typeof m === 'function', 'needs-auth() is available')
})

test('a scoped package that need authentication specific to scope', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  const client = new RegClient()

  const data = await new Promise((resolve, reject) => {
    client.adduser('http://localhost:4873', {
      auth: {
        email: 'foo@bar.com',
        password: 'bar',
        username: 'foo',
      },
    }, (err: Error, d: { token: string }) => err ? reject(err) : resolve(d))
  }) as {token: string}

  const rawConfig = {
    '//localhost:4873/:_authToken': data.token,
    '@private:registry': 'http://localhost:4873/',
    'registry': 'https://registry.npmjs.org/',
  }
  let opts = await testDefaults({}, {
    rawConfig,
    registry: 'https://registry.npmjs.org/',
  }, {
    rawConfig,
  })
  const manifest = await addDependenciesToPackage({}, ['@private/foo'], opts)

  await project.has('@private/foo')

  // should work when a lockfile is available
  await rimraf('node_modules')
  await rimraf(path.join('..', '.store'))

  // Recreating options to have a new storeController with clean cache
  opts = await testDefaults({}, {
    rawConfig,
    registry: 'https://registry.npmjs.org/',
  }, {
    rawConfig,
  })
  await addDependenciesToPackage(manifest, ['@private/foo'], opts)

  await project.has('@private/foo')
})

test('a package that need authentication reuses authorization tokens for tarball fetching', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  const client = new RegClient()

  const data = await new Promise((resolve, reject) => {
    client.adduser('http://localhost:4873', {
      auth: {
        email: 'foo@bar.com',
        password: 'bar',
        username: 'foo',
      },
    }, (err: Error, d: { token: string }) => err ? reject(err) : resolve(d))
  }) as {token: string}

  const rawConfig = {
    '//127.0.0.1:4873/:_authToken': data.token,
    '//127.0.0.1:4873/:always-auth': true,
    'registry': 'http://127.0.0.1:4873',
  }
  await addDependenciesToPackage({}, ['needs-auth'], await testDefaults({
    registries: {
      default: 'http://127.0.0.1:4873',
    },
  }, {
    rawConfig,
    registry: 'http://127.0.0.1:4873',
  }, {
    rawConfig,
  }))

  const m = project.requireModule('needs-auth')

  t.ok(typeof m === 'function', 'needs-auth() is available')
})

test('a package that need authentication reuses authorization tokens for tarball fetching when meta info is cached', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  const client = new RegClient()

  const data = await new Promise((resolve, reject) => {
    client.adduser('http://localhost:4873', {
      auth: {
        email: 'foo@bar.com',
        password: 'bar',
        username: 'foo',
      },
    }, (err: Error, d: { token: string }) => err ? reject(err) : resolve(d))
  }) as {token: string}

  const rawConfig = {
    '//127.0.0.1:4873/:_authToken': data.token,
    '//127.0.0.1:4873/:always-auth': true,
    'registry': 'http://127.0.0.1:4873',
  }
  let opts = await testDefaults({
    registries: {
      default: 'http://127.0.0.1:4873',
    },
  }, {
    rawConfig,
    registry: 'http://127.0.0.1:4873',
  }, {
    rawConfig,
  })

  const manifest = await addDependenciesToPackage({}, ['needs-auth'], opts)

  await rimraf('node_modules')
  await rimraf(path.join('..', '.registry'))
  await rimraf(path.join('..', '.store'))

  // Recreating options to clean store cache
  opts = await testDefaults({
    registries: {
      default: 'http://127.0.0.1:4873',
    },
  }, {
    rawConfig,
    registry: 'http://127.0.0.1:4873',
  }, {
    rawConfig,
  })
  await install(manifest, opts)

  const m = project.requireModule('needs-auth')

  t.ok(typeof m === 'function', 'needs-auth() is available')
})
