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

  await installPkgs(['needs-auth'], testDefaults({
    rawNpmConfig: {
      '//localhost:4873/:_authToken': data['token'],
    },
  }))

  const m = project.requireModule('needs-auth')

  t.ok(typeof m === 'function', 'needs-auth() is available')

  // should work when a shrinkwrap is available
  // and the registry in .npmrc is not the same as the one in shrinkwrap
  await rimraf('node_modules')
  await rimraf(path.join('..', '.store'))

  await installPkgs(['needs-auth'], testDefaults({
    registry: 'https://registry.npmjs.org/',
    rawNpmConfig: {
      registry: 'https://registry.npmjs.org/',
      '//localhost:4873/:_authToken': data['token'],
    },
  }))

  project.has('needs-auth')
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

  await installPkgs(['needs-auth'], testDefaults({
    rawNpmConfig: {
      '_auth': 'Zm9vOmJhcg==', // base64 encoded foo:bar
      'always-auth': true,
      registry: 'http://localhost:4873',
    },
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

  const opts = testDefaults({
    registry: 'https://registry.npmjs.org/',
    rawNpmConfig: {
      registry: 'https://registry.npmjs.org/',
      '@private:registry': 'http://localhost:4873/',
      '//localhost:4873/:_authToken': data['token'],
    },
  })
  await installPkgs(['@private/foo'], opts)

  project.has('@private/foo')

  // should work when a shrinkwrap is available
  await rimraf('node_modules')
  await rimraf(path.join('..', '.store'))

  await installPkgs(['@private/foo'], opts)

  project.has('@private/foo')
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

  await installPkgs(['needs-auth'], testDefaults({
    registry: 'http://127.0.0.1:4873',
    rawNpmConfig: {
      '//127.0.0.1:4873/:_authToken': data['token'],
      '//127.0.0.1:4873/:always-auth': true,
    },
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

  const opts = testDefaults({
    registry: 'http://127.0.0.1:4873',
    rawNpmConfig: {
      '//127.0.0.1:4873/:_authToken': data['token'],
      '//127.0.0.1:4873/:always-auth': true,
    },
  })

  await installPkgs(['needs-auth'], opts)

  await rimraf('node_modules')
  await rimraf(path.join('..', '.registry'))
  await rimraf(path.join('..', '.store'))

  await install(opts)

  const m = project.requireModule('needs-auth')

  t.ok(typeof m === 'function', 'needs-auth() is available')
})
