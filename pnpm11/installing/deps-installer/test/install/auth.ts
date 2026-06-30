import http from 'node:http'
import type { AddressInfo } from 'node:net'
import path from 'node:path'

import { test } from '@jest/globals'
import { addDependenciesToPackage, install } from '@pnpm/installing.deps-installer'
import { prepareEmpty } from '@pnpm/prepare'
import { addUser, getRegistryMockToken, REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
import type { RegistryConfig } from '@pnpm/types'
import { rimrafSync } from '@zkochan/rimraf'

import { testDefaults } from '../utils/index.js'

const skipOnNode17 = ['v14', 'v16'].includes(process.version.split('.')[0]) ? test : test.skip

const BASIC_AUTH_CREDENTIALS = { username: 'foo', password: 'bar' }
const EXPECTED_BASIC_AUTH = `Basic ${Buffer.from(
  `${BASIC_AUTH_CREDENTIALS.username}:${BASIC_AUTH_CREDENTIALS.password}`
).toString('base64')}`

// pnpr only accepts bearer tokens, so it can't exercise pnpm's HTTP Basic
// (`_auth`) client support directly. This stands up a registry that does:
// it rejects requests lacking the expected Basic credentials with 401, and
// otherwise forwards to pnpr with a bearer token. Packument tarball URLs
// (rewritten by pnpr to its `public_url`) are pointed back at this proxy so
// tarball fetches go through the same Basic-auth boundary.
async function withBasicAuthRegistry (run: (registryUrl: string) => Promise<void>): Promise<void> {
  const upstreamBase = `http://localhost:${REGISTRY_MOCK_PORT}`
  const bearer = `Bearer ${getRegistryMockToken()}`
  let proxyBase = ''
  const server = http.createServer((req, res) => {
    void (async () => {
      if (req.headers.authorization !== EXPECTED_BASIC_AUTH) {
        res.writeHead(401, { 'www-authenticate': 'Basic realm="pnpr"' })
        res.end('Unauthorized')
        return
      }
      const upstream = await fetch(`${upstreamBase}${req.url}`, {
        method: req.method,
        headers: { accept: req.headers.accept ?? '*/*', authorization: bearer },
      })
      const contentType = upstream.headers.get('content-type') ?? ''
      if (contentType.includes('json')) {
        const body = (await upstream.text()).split(upstreamBase).join(proxyBase)
        res.writeHead(upstream.status, { 'content-type': 'application/json' })
        res.end(body)
      } else {
        res.writeHead(upstream.status, { 'content-type': contentType })
        res.end(Buffer.from(await upstream.arrayBuffer()))
      }
    })().catch((err: unknown) => {
      res.writeHead(500)
      res.end(String(err))
    })
  })
  await new Promise<void>((resolve) => {
    server.listen(0, resolve)
  })
  proxyBase = `http://localhost:${(server.address() as AddressInfo).port}`
  try {
    await run(`${proxyBase}/`)
  } finally {
    server.closeAllConnections()
    await new Promise<void>((resolve, reject) => {
      server.close((err) => {
        if (err == null) {
          resolve()
        } else {
          reject(err)
        }
      })
    })
  }
}

test('a package that need authentication', async () => {
  const project = prepareEmpty()

  const data = await addUser({
    email: 'foo@bar.com',
    password: 'bar',
    username: 'foo',
  })

  let configByUri: Record<string, RegistryConfig> = {
    [`//localhost:${REGISTRY_MOCK_PORT}/`]: { '@': { authToken: data.token } },
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
    [`//localhost:${REGISTRY_MOCK_PORT}/`]: { '@': { authToken: data.token } },
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

  await withBasicAuthRegistry(async (registry) => {
    const configByUri: Record<string, RegistryConfig> = {
      [registry.replace(/^https?:/, '')]: { '@': { basicAuth: BASIC_AUTH_CREDENTIALS } },
    }
    await addDependenciesToPackage({}, ['@pnpm.e2e/needs-auth'], testDefaults({
      registries: { default: registry },
    }, {
      configByUri,
      registry,
    }, {
      configByUri,
    }))

    project.has('@pnpm.e2e/needs-auth')
  })
})

test('a scoped package that need authentication specific to scope', async () => {
  const project = prepareEmpty()

  const data = await addUser({
    email: 'foo@bar.com',
    password: 'bar',
    username: 'foo',
  })

  const configByUri: Record<string, RegistryConfig> = {
    [`//localhost:${REGISTRY_MOCK_PORT}/`]: { '@': { authToken: data.token } },
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

  await withBasicAuthRegistry(async (registry) => {
    const configByUri: Record<string, RegistryConfig> = {
      [registry.replace(/^https?:/, '')]: { '@': { basicAuth: BASIC_AUTH_CREDENTIALS } },
    }
    let opts = testDefaults({
      registries: {
        default: 'https://registry.npmjs.org/',
        '@private': registry,
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
        '@private': registry,
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
})

skipOnNode17('a package that need authentication reuses authorization tokens for tarball fetching', async () => {
  const project = prepareEmpty()

  const data = await addUser({
    email: 'foo@bar.com',
    password: 'bar',
    username: 'foo',
  })

  const configByUri: Record<string, RegistryConfig> = {
    [`//127.0.0.1:${REGISTRY_MOCK_PORT}/`]: { '@': { authToken: data.token } },
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
    [`//127.0.0.1:${REGISTRY_MOCK_PORT}/`]: { '@': { authToken: data.token } },
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
