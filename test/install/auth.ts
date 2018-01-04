import tape = require('tape')
import promisifyTape from 'tape-promise'
import path = require('path')
import {
  prepare,
  testDefaults,
} from '../utils'
import {installPkgs, install} from 'supi'
import registryMock = require('pnpm-registry-mock')
import RegClient = require('anonymous-npm-registry-client')
import rimraf = require('rimraf-then')

const test = promisifyTape(tape)

test('a package that need authentication', async function (t: tape.Test) {
  const project = prepare(t)

  const client = new RegClient()

  const data = await new Promise((resolve, reject) => {
    client.adduser('http://localhost:4873', {
      auth: {
        username: 'foo',
        password: 'bar',
        email: 'foo@bar.com',
      }
    }, (err: Error, data: Object) => err ? reject(err) : resolve(data))
  })

  let rawNpmConfig = {
    registry: 'http://localhost:4873/',
    '//localhost:4873/:_authToken': data['token'],
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
    registry: 'https://registry.npmjs.org/',
    '//localhost:4873/:_authToken': data['token'],
  }
  await installPkgs(['needs-auth'], await testDefaults({}, {
    registry: 'https://registry.npmjs.org/',
    rawNpmConfig,
  }, {
    rawNpmConfig,
  }))

  await project.has('needs-auth')
})

test('a package that need authentication, legacy way', async function (t: tape.Test) {
  const project = prepare(t)

  const client = new RegClient()

  const data = await new Promise((resolve, reject) => {
    client.adduser('http://localhost:4873', {
      auth: {
        username: 'foo',
        password: 'bar',
        email: 'foo@bar.com',
      }
    }, (err: Error, data: Object) => err ? reject(err) : resolve(data))
  })

  const rawNpmConfig = {
    '_auth': 'Zm9vOmJhcg==', // base64 encoded foo:bar
    'always-auth': true,
    registry: 'http://localhost:4873',
  }
  await installPkgs(['needs-auth'], await testDefaults({}, {
    rawNpmConfig,
  }, {
    rawNpmConfig,
  }))

  const m = project.requireModule('needs-auth')

  t.ok(typeof m === 'function', 'needs-auth() is available')
})

test('a scoped package that need authentication specific to scope', async function (t: tape.Test) {
  const project = prepare(t)

  const client = new RegClient()

  const data = await new Promise((resolve, reject) => {
    client.adduser('http://localhost:4873', {
      auth: {
        username: 'foo',
        password: 'bar',
        email: 'foo@bar.com',
      }
    }, (err: Error, data: Object) => err ? reject(err) : resolve(data))
  })

  const rawNpmConfig = {
    registry: 'https://registry.npmjs.org/',
    '@private:registry': 'http://localhost:4873/',
    '//localhost:4873/:_authToken': data['token'],
  }
  let opts = await testDefaults({}, {
    registry: 'https://registry.npmjs.org/',
    rawNpmConfig,
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
    registry: 'https://registry.npmjs.org/',
    rawNpmConfig,
  }, {
    rawNpmConfig,
  })
  await installPkgs(['@private/foo'], opts)

  await project.has('@private/foo')
})

test('a package that need authentication reuses authorization tokens for tarball fetching', async function (t: tape.Test) {
  const project = prepare(t)

  const client = new RegClient()

  const data = await new Promise((resolve, reject) => {
    client.adduser('http://localhost:4873', {
      auth: {
        username: 'foo',
        password: 'bar',
        email: 'foo@bar.com',
      }
    }, (err: Error, data: Object) => err ? reject(err) : resolve(data))
  })

  const rawNpmConfig = {
    registry: 'http://127.0.0.1:4873',
    '//127.0.0.1:4873/:_authToken': data['token'],
    '//127.0.0.1:4873/:always-auth': true,
  }
  await installPkgs(['needs-auth'], await testDefaults({
    registry: 'http://127.0.0.1:4873',
  }, {
    registry: 'http://127.0.0.1:4873',
    rawNpmConfig,
  }, {
    rawNpmConfig,
  }))

  const m = project.requireModule('needs-auth')

  t.ok(typeof m === 'function', 'needs-auth() is available')
})

test('a package that need authentication reuses authorization tokens for tarball fetching when meta info is cached', async function (t: tape.Test) {
  const project = prepare(t)

  const client = new RegClient()

  const data = await new Promise((resolve, reject) => {
    client.adduser('http://localhost:4873', {
      auth: {
        username: 'foo',
        password: 'bar',
        email: 'foo@bar.com',
      }
    }, (err: Error, data: Object) => err ? reject(err) : resolve(data))
  })

  const rawNpmConfig = {
    registry: 'http://127.0.0.1:4873',
    '//127.0.0.1:4873/:_authToken': data['token'],
    '//127.0.0.1:4873/:always-auth': true,
  }
  let opts = await testDefaults({
    registry: 'http://127.0.0.1:4873',
  }, {
    registry: 'http://127.0.0.1:4873',
    rawNpmConfig,
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
    registry: 'http://127.0.0.1:4873',
    rawNpmConfig,
  }, {
    rawNpmConfig,
  })
  await install(opts)

  const m = project.requireModule('needs-auth')

  t.ok(typeof m === 'function', 'needs-auth() is available')
})
