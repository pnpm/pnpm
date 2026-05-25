import { createFetchFromRegistry } from '@pnpm/network.fetch'
import { setDistTag } from '@pnpm/registry-access.set-dist-tag'
import { REGISTRY_MOCK_CREDENTIALS, REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'

const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}/`

const AUTH_HEADER = `Basic ${Buffer.from(
  `${REGISTRY_MOCK_CREDENTIALS.username}:${REGISTRY_MOCK_CREDENTIALS.password}`
).toString('base64')}`

const fetchFromRegistry = createFetchFromRegistry({})

export interface AddDistTagOptions {
  package: string
  version: string
  distTag: string
}

export async function addDistTag (opts: AddDistTagOptions): Promise<void> {
  await setDistTag({
    packageName: opts.package,
    version: opts.version,
    distTag: opts.distTag,
    registryUrl: REGISTRY_URL,
    authHeader: AUTH_HEADER,
    fetchFromRegistry,
  })
}
