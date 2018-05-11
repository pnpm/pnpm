import RegClient = require('anonymous-npm-registry-client')
import path = require('path')
import registryMock = require('pnpm-registry-mock')
import rimraf = require('rimraf-then')
import {install, installPkgs} from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import {
  prepare,
  testDefaults,
} from '../utils'

const test = promisifyTape(tape)

test('a package that need authentication', async (t: tape.Test) => {
  const project = prepare(t)

  const client = new RegClient()

  const data = await new Promise((resolve, reject) => {
    client.adduser('http://localhost:4873', {
      auth: {
        email: 'foo@bar.com',
        password: 'bar',
        username: 'foo',
      },
    }, (err: Error, d: object) => err ? reject(err) : resolve(d))
  }) as {token: string}

  let rawNpmConfig = {
    '//localhost:4873/:_authToken': data.token,
    'registry': 'http://localhost:4873/',
  }
  await installPkgs(['needs-auth'], await testDefaults({}, {
    rawNpmConfig,
  }, {
    rawNpmConfig,
  }))

  const m = project.requireModule('needs-auth')

  t.ok(typeof m === 'function', 'needs-auth() is available')

  // should work when a shrinkwrap is available
  // and the registry in .npmrc is not the same as the one in shrinkwrap
  await rimraf('node_modules')
  await rimraf(path.join('..', '.store'))

  rawNpmConfig = {
    '//localhost:4873/:_authToken': data.token,
    'registry': 'https://registry.npmjs.org/',
  }
  await installPkgs(['needs-auth'], await testDefaults({}, {
    rawNpmConfig,
    registry: 'https://registry.npmjs.org/',
  }, {
    rawNpmConfig,
  }))

  await project.has('needs-auth')
})

test('a package that need authentication, legacy way', async (t: tape.Test) => {
  const project = prepare(t)

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

  const rawNpmConfig = {
    '_auth': 'Zm9vOmJhcg==', // base64 encoded foo:bar
    'always-auth': true,
    'registry': 'http://localhost:4873',
  }
  await installPkgs(['needs-auth'], await testDefaults({}, {
    rawNpmConfig,
  }, {
    rawNpmConfig,
  }))

  const m = project.requireModule('needs-auth')

  t.ok(typeof m === 'function', 'needs-auth() is available')
})

test('a scoped package that need authentication specific to scope', async (t: tape.Test) => {
  const project = prepare(t)

  const client = new RegClient()

  const data = await new Promise((resolve, reject) => {
    client.adduser('http://localhost:4873', {
      auth: {
        email: 'foo@bar.com',
        password: 'bar',
        username: 'foo',
      },
    }, (err: Error, d: object) => err ? reject(err) : resolve(d))
  }) as {token: string}

  const rawNpmConfig = {
    '//localhost:4873/:_authToken': data.token,
    '@private:registry': 'http://localhost:4873/',
    'registry': 'https://registry.npmjs.org/',
  }
  let opts = await testDefaults({}, {
    rawNpmConfig,
    registry: 'https://registry.npmjs.org/',
  }, {
    rawNpmConfig,
  })
  await installPkgs(['@private/foo'], opts)

  await project.has('@private/foo')

  // should work when a shrinkwrap is available
  await rimraf('node_modules')
  await rimraf(path.join('..', '.store'))

  // Recreating options to have a new storeController with clean cache
  opts = await testDefaults({}, {
    rawNpmConfig,
    registry: 'https://registry.npmjs.org/',
  }, {
    rawNpmConfig,
  })
  await installPkgs(['@private/foo'], opts)

  await project.has('@private/foo')
})

test('a package that need authentication reuses authorization tokens for tarball fetching', async (t: tape.Test) => {
  const project = prepare(t)

  const client = new RegClient()

  const data = await new Promise((resolve, reject) => {
    client.adduser('http://localhost:4873', {
      auth: {
        email: 'foo@bar.com',
        password: 'bar',
        username: 'foo',
      },
    }, (err: Error, d: object) => err ? reject(err) : resolve(d))
  }) as {token: string}

  const rawNpmConfig = {
    '//127.0.0.1:4873/:_authToken': data.token,
    '//127.0.0.1:4873/:always-auth': true,
    'registry': 'http://127.0.0.1:4873',
  }
  await installPkgs(['needs-auth'], await testDefaults({
    registry: 'http://127.0.0.1:4873',
  }, {
    rawNpmConfig,
    registry: 'http://127.0.0.1:4873',
  }, {
    rawNpmConfig,
  }))

  const m = project.requireModule('needs-auth')

  t.ok(typeof m === 'function', 'needs-auth() is available')
})

test('a package that need authentication reuses authorization tokens for tarball fetching when meta info is cached', async (t: tape.Test) => {
  const project = prepare(t)

  const client = new RegClient()

  const data = await new Promise((resolve, reject) => {
    client.adduser('http://localhost:4873', {
      auth: {
        email: 'foo@bar.com',
        password: 'bar',
        username: 'foo',
      },
    }, (err: Error, d: object) => err ? reject(err) : resolve(d))
  }) as {token: string}

  const rawNpmConfig = {
    '//127.0.0.1:4873/:_authToken': data.token,
    '//127.0.0.1:4873/:always-auth': true,
    'registry': 'http://127.0.0.1:4873',
  }
  let opts = await testDefaults({
    registry: 'http://127.0.0.1:4873',
  }, {
    rawNpmConfig,
    registry: 'http://127.0.0.1:4873',
  }, {
    rawNpmConfig,
  })

  await installPkgs(['needs-auth'], opts)

  await rimraf('node_modules')
  await rimraf(path.join('..', '.registry'))
  await rimraf(path.join('..', '.store'))

  // Recreating options to clean store cache
  opts = await testDefaults({
    registry: 'http://127.0.0.1:4873',
  }, {
    rawNpmConfig,
    registry: 'http://127.0.0.1:4873',
  }, {
    rawNpmConfig,
  })
  await install(opts)

  const m = project.requireModule('needs-auth')

  t.ok(typeof m === 'function', 'needs-auth() is available')
})
