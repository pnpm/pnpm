import { afterAll, beforeAll, describe, expect, jest, test } from '@jest/globals'

// Mock `ci-info` so `GITHUB_ACTIONS` is false: that way `getIdToken` short-circuits in
// `fetchTokenAndProvenanceByOidc` and `createPublishOptions` never touches the network.
const ciInfoModule = await import('ci-info')
const ciInfoOriginal = (ciInfoModule as { default?: Record<string, unknown> }).default ?? ciInfoModule
jest.unstable_mockModule('ci-info', () => ({
  ...ciInfoModule,
  default: { ...ciInfoOriginal, GITHUB_ACTIONS: false },
  GITHUB_ACTIONS: false,
}))

const { createPublishOptions } = await import('../../src/publish/publishPackedPkg.js')

function baseOpts (): Parameters<typeof createPublishOptions>[1] {
  return {
    configByUri: {},
    fetchTimeout: 60_000,
    registries: { default: 'https://registry.npmjs.org/' },
  } as Parameters<typeof createPublishOptions>[1]
}

// `getIdToken` honors `NPM_ID_TOKEN` regardless of which CI flag is set, so make sure
// it isn't set during these tests — otherwise OIDC would attempt a token exchange.
let savedNpmIdToken: string | undefined
beforeAll(() => {
  savedNpmIdToken = process.env['NPM_ID_TOKEN']
  delete process.env['NPM_ID_TOKEN']
})
afterAll(() => {
  if (savedNpmIdToken === undefined) return
  process.env['NPM_ID_TOKEN'] = savedNpmIdToken
})

describe('createPublishOptions: strictSSL', () => {
  test('forwards strictSsl: false as strictSSL to npm-registry-fetch', async () => {
    const opts = await createPublishOptions(
      { name: 'pkg', version: '1.0.0' },
      { ...baseOpts(), strictSsl: false }
    )
    expect(opts.strictSSL).toBe(false)
  })

  test('strictSSL is absent when strictSsl is not set', async () => {
    const opts = await createPublishOptions(
      { name: 'pkg', version: '1.0.0' },
      baseOpts()
    )
    expect(opts.strictSSL).toBeUndefined()
  })
})

describe('createPublishOptions: access', () => {
  test('falls back to publishConfig.access when --access is not set', async () => {
    const opts = await createPublishOptions(
      { name: '@scope/pkg', version: '1.0.0', publishConfig: { access: 'restricted' } },
      baseOpts()
    )
    expect(opts.access).toBe('restricted')
  })

  test('CLI --access wins over publishConfig.access', async () => {
    const opts = await createPublishOptions(
      { name: '@scope/pkg', version: '1.0.0', publishConfig: { access: 'restricted' } },
      { ...baseOpts(), access: 'public' }
    )
    expect(opts.access).toBe('public')
  })

  test('access defaults to null when neither CLI nor publishConfig set it', async () => {
    const opts = await createPublishOptions(
      { name: '@scope/pkg', version: '1.0.0' },
      baseOpts()
    )
    expect(opts.access).toBeNull()
  })

  test('invalid publishConfig.access values fall back to default access', async () => {
    const opts = await createPublishOptions(
      { name: '@scope/pkg', version: '1.0.0', publishConfig: { access: 'bogus' as 'public' } },
      baseOpts()
    )
    expect(opts.access).toBeNull()
  })
})

describe('createPublishOptions: auth', () => {
  test('prefers package-scoped credentials over registry-wide credentials', async () => {
    const opts = await createPublishOptions(
      { name: '@scope/pkg', version: '1.0.0' },
      {
        ...baseOpts(),
        configByUri: {
          '//registry.npmjs.org/': {
            '@': { authToken: 'default-token' },
            '@scope': { authToken: 'scoped-token' },
          },
        },
      },
      { oidc: false }
    )

    expect(opts.token).toBe('scoped-token')
  })
})
