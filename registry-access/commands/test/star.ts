import { createFetchFromRegistry } from '@pnpm/network.fetch'
import npa from '@pnpm/npm-package-arg'
import { prepare } from '@pnpm/prepare'
import { star, unstar, stars, whoami } from '@pnpm/registry-access.commands'
import { REGISTRY_MOCK_CREDENTIALS, REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { publish } from '@pnpm/releasing.commands'
import { DEFAULT_OPTS as BASE_OPTS } from '@pnpm/testing.command-defaults'

const DEFAULT_OPTS = {
  ...BASE_OPTS,
  bail: false,
}

const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}`

const CONFIG_BY_URI = {
  [`//localhost:${REGISTRY_MOCK_PORT}/`]: {
    creds: {
      basicAuth: REGISTRY_MOCK_CREDENTIALS,
    },
  },
}

async function publishVersion (name: string, version: string): Promise<void> {
  prepare({
    name,
    version,
  })

  await publish.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['publish'] },
    configByUri: CONFIG_BY_URI,
    dir: process.cwd(),
    registries: { default: REGISTRY },
  }, [])
}

async function getPackageUsers (pkgName: string): Promise<Record<string, boolean>> {
  const fetchFromRegistry = createFetchFromRegistry({
    configByUri: CONFIG_BY_URI,
  })
  const encodedName = npa(pkgName).escapedName
  const response = await fetchFromRegistry(`${REGISTRY}/${encodedName}`, {
    authHeaderValue: `Basic ${REGISTRY_MOCK_CREDENTIALS}`,
  })
  const pkgData = await response.json() as { users?: Record<string, boolean> }
  return pkgData.users || {}
}

test('whoami: should return the current user', async () => {
  const result = await whoami.handler({
    ...DEFAULT_OPTS,
    configByUri: CONFIG_BY_URI,
    registries: { default: REGISTRY },
  })

  expect(result).toBe('username')
})

test('star/unstar: should star and unstar a package', async () => {
  const pkgName = 'test-star-package'
  await publishVersion(pkgName, '0.0.1')

  await star.handler({
    ...DEFAULT_OPTS,
    configByUri: CONFIG_BY_URI,
    registries: { default: REGISTRY },
  }, [pkgName])

  let users = await getPackageUsers(pkgName)
  // We use .toBeTruthy() as some registries might return different values for 'starred'
  expect(users['username']).toBeTruthy()

  await unstar.handler({
    ...DEFAULT_OPTS,
    configByUri: CONFIG_BY_URI,
    registries: { default: REGISTRY },
  }, [pkgName])

  users = await getPackageUsers(pkgName)
  expect(users['username']).toBeFalsy()
})

test('stars: should list starred packages for current user', async () => {
  const pkgName1 = 'test-stars-pkg-1'
  const pkgName2 = 'test-stars-pkg-2'
  await publishVersion(pkgName1, '0.0.1')
  await publishVersion(pkgName2, '0.0.1')

  await star.handler({
    ...DEFAULT_OPTS,
    configByUri: CONFIG_BY_URI,
    registries: { default: REGISTRY },
  }, [pkgName1])

  await star.handler({
    ...DEFAULT_OPTS,
    configByUri: CONFIG_BY_URI,
    registries: { default: REGISTRY },
  }, [pkgName2])

  const result = await stars.handler({
    ...DEFAULT_OPTS,
    configByUri: CONFIG_BY_URI,
    registries: { default: REGISTRY },
  }, [])

  expect(result).toContain(pkgName1)
  expect(result).toContain(pkgName2)
})

test('star: should throw when package name is not provided', async () => {
  await expect(async () => {
    await star.handler({
      ...DEFAULT_OPTS,
      configByUri: CONFIG_BY_URI,
      registries: { default: REGISTRY },
    }, [])
  }).rejects.toThrow('Package name is required')
})

test('unstar: should throw when package name is not provided', async () => {
  await expect(async () => {
    await unstar.handler({
      ...DEFAULT_OPTS,
      configByUri: CONFIG_BY_URI,
      registries: { default: REGISTRY },
    }, [])
  }).rejects.toThrow('Package name is required')
})
