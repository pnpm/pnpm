import { describe, expect, it, jest } from '@jest/globals'

// Mock `@pnpm/network.fetch` so we can spy on the options that
// `createPublishContext` forwards to `createDispatchedFetch`. This is the
// wiring that fixes https://github.com/pnpm/pnpm/issues/11561.
const createDispatchedFetchMock = jest.fn<(opts: Record<string, unknown>) => () => Promise<Response>>(() => () => Promise.resolve(new Response()))
const realNetworkFetch = await import('@pnpm/network.fetch')
jest.unstable_mockModule('@pnpm/network.fetch', () => ({
  ...realNetworkFetch,
  createDispatchedFetch: createDispatchedFetchMock,
}))

const { createPublishContext } = await import('../../src/publish/publishPackedPkg.js')

function baseOpts (): Parameters<typeof createPublishContext>[0] {
  return {
    configByUri: {},
    fetchTimeout: 60_000,
    registries: { default: 'https://registry.npmjs.org/' },
  } as Parameters<typeof createPublishContext>[0]
}

describe('createPublishContext', () => {
  it('forwards proxy / TLS / local-address settings to createDispatchedFetch', () => {
    createDispatchedFetchMock.mockClear()
    createPublishContext({
      ...baseOpts(),
      httpProxy: 'http://proxy.example:8080',
      httpsProxy: 'http://proxy.example:1234',
      noProxy: 'localhost,127.0.0.1',
      localAddress: '10.0.0.1',
      strictSsl: false,
      ca: 'ca-pem',
      cert: 'cert-pem',
      key: 'key-pem',
    })
    expect(createDispatchedFetchMock).toHaveBeenCalledTimes(1)
    expect(createDispatchedFetchMock.mock.calls[0][0]).toMatchObject({
      httpProxy: 'http://proxy.example:8080',
      httpsProxy: 'http://proxy.example:1234',
      noProxy: 'localhost,127.0.0.1',
      localAddress: '10.0.0.1',
      strictSsl: false,
      ca: 'ca-pem',
      cert: 'cert-pem',
      key: 'key-pem',
    })
  })

  it('translates fetchTimeout to timeout', () => {
    createDispatchedFetchMock.mockClear()
    createPublishContext({ ...baseOpts(), fetchTimeout: 12_345 })
    expect(createDispatchedFetchMock.mock.calls[0][0]).toMatchObject({ timeout: 12_345 })
  })

  it('forwards configByUri so registry-scoped TLS settings reach the dispatcher', () => {
    createDispatchedFetchMock.mockClear()
    const configByUri = { '//my-registry.example/': { tls: { cert: 'c', key: 'k' } } }
    createPublishContext({ ...baseOpts(), configByUri })
    expect(createDispatchedFetchMock.mock.calls[0][0]).toMatchObject({ configByUri })
  })

  it('produces a context.fetch that delegates to the dispatched fetch', () => {
    const dispatchedFetch = jest.fn<() => Promise<Response>>(() => Promise.resolve(new Response()))
    createDispatchedFetchMock.mockReturnValueOnce(dispatchedFetch as ReturnType<typeof createDispatchedFetchMock>)
    const ctx = createPublishContext(baseOpts())
    expect(ctx.fetch).toBe(dispatchedFetch)
  })
})
