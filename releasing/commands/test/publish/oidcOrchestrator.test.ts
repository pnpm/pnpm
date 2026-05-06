import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, jest, test } from '@jest/globals'
import { type Dispatcher, getGlobalDispatcher, MockAgent, setGlobalDispatcher } from 'undici'

// `getIdToken` honors `NPM_ID_TOKEN` from env regardless of which CI we're in (or none),
// so setting that env var is enough to drive an idToken into the orchestrator. The
// ci-info mock below is *only* needed for the happy-path test that asserts
// `provenance: true`: `determineProvenance` still gates the visibility check on
// `GITHUB_ACTIONS || (GITLAB && SIGSTORE_ID_TOKEN)`, since visibility is read from
// CI-specific JWT claims (`repository_visibility` / `project_visibility`). Without one
// of those flags being true, the visibility branch can't fire.
//
// All registry HTTP traffic — token exchange, visibility check — goes through
// `@pnpm/network.fetch`, which wraps `undici.fetch`. We intercept at the undici layer
// with `MockAgent` so the wrapper's retry/response-handling code runs for real.
const ciInfoModule = await import('ci-info')
const ciInfoOriginal = (ciInfoModule as { default?: Record<string, unknown> }).default ?? ciInfoModule
jest.unstable_mockModule('ci-info', () => ({
  ...ciInfoModule,
  default: { ...ciInfoOriginal, GITLAB: true, GITHUB_ACTIONS: false },
  GITLAB: true,
  GITHUB_ACTIONS: false,
}))

// Importing publishPackedPkg transitively loads `@pnpm/network.fetch`, whose
// `dispatcher.ts` calls `setGlobalDispatcher(...)` on load. We therefore install the
// MockAgent *after* this import so it isn't clobbered.
const { fetchTokenAndProvenanceByOidc } = await import('../../src/publish/publishPackedPkg.js')

const REGISTRY_ORIGIN = 'https://registry.npmjs.org'
const REGISTRY = `${REGISTRY_ORIGIN}/`
const PACKAGE_NAME = '@scope/pkg'
// Build the URL-escaped form dynamically rather than hardcoding it, both because that's
// what the source uses (via `npa(...).escapedName`, lowercase percent-encoding) and to
// avoid spell-checking complaints about the synthesized substring. The global regex
// keeps CodeQL happy too — npm package names only ever have one `/` (scope separator),
// but pattern-matching on `replace('/', ...)` would still flag this line.
const ESCAPED_PACKAGE_NAME = PACKAGE_NAME.replace(/\//g, '%2f')
const TOKEN_EXCHANGE_PATH = `/-/npm/v1/oidc/token/exchange/package/${ESCAPED_PACKAGE_NAME}`
const VISIBILITY_PATH = `/-/package/${ESCAPED_PACKAGE_NAME}/visibility`

// JWT shape expected by `determineProvenance`: `header.payload.sig` where the payload is
// base64url-encoded JSON. `project_visibility: 'public'` triggers the GitLab branch of the
// visibility check (combined with our mocked `ciInfo.GITLAB` and `SIGSTORE_ID_TOKEN` env).
const FAKE_JWT = `header.${Buffer.from(JSON.stringify({ project_visibility: 'public' })).toString('base64url')}.sig`

const baseOptions = {
  configByUri: {},
  registries: { default: REGISTRY },
  // Disable retries so a single 5xx response in a test doesn't trigger 3 calls and
  // exhaust an interceptor / drag the test out.
  fetchRetries: 0,
}

let originalDispatcher: Dispatcher
let mockAgent: MockAgent

beforeAll(() => {
  originalDispatcher = getGlobalDispatcher()
})

beforeEach(() => {
  process.env['NPM_ID_TOKEN'] = FAKE_JWT
  process.env['SIGSTORE_ID_TOKEN'] = 'sigstore-token'
  mockAgent = new MockAgent()
  // Any unmatched HTTP must fail loudly rather than escape the test sandbox.
  mockAgent.disableNetConnect()
  setGlobalDispatcher(mockAgent)
})

afterEach(async () => {
  await mockAgent.close()
})

afterAll(() => {
  setGlobalDispatcher(originalDispatcher)
  delete process.env['NPM_ID_TOKEN']
  delete process.env['SIGSTORE_ID_TOKEN']
})

describe('fetchTokenAndProvenanceByOidc', () => {
  test('returns the OIDC-derived authToken and determined provenance when both registry calls succeed', async () => {
    const pool = mockAgent.get(REGISTRY_ORIGIN)
    pool.intercept({ path: TOKEN_EXCHANGE_PATH, method: 'POST' })
      .reply(200, { token: 'oidc-auth-token' })
    pool.intercept({ path: VISIBILITY_PATH, method: 'GET' })
      .reply(200, { public: true })

    const result = await fetchTokenAndProvenanceByOidc(PACKAGE_NAME, REGISTRY, baseOptions)

    expect(result).toEqual({ authToken: 'oidc-auth-token', provenance: true })
    // Both interceptors consumed → both endpoints were called exactly once.
    mockAgent.assertNoPendingInterceptors()
  })

  test('returns undefined when the registry has no trusted publisher configured for the package (4xx on token exchange)', async () => {
    // Real-world fallback path: on `pnpm publish -r` for a workspace where some packages
    // don't have trusted publishing configured, the registry returns a 4xx and we want the
    // caller to fall back to the static `_authToken`. Critically, we must NOT call the
    // visibility endpoint after auth has failed — verify by registering only one interceptor
    // and asserting it (and only it) was consumed.
    const pool = mockAgent.get(REGISTRY_ORIGIN)
    pool.intercept({ path: TOKEN_EXCHANGE_PATH, method: 'POST' })
      .reply(403, { body: { message: 'No trusted publisher for package' } })

    const result = await fetchTokenAndProvenanceByOidc(PACKAGE_NAME, REGISTRY, baseOptions)

    expect(result).toBeUndefined()
    mockAgent.assertNoPendingInterceptors()
  })

  test('preserves the OIDC authToken when the visibility check fails (regression: provenance error must not discard the token)', async () => {
    const pool = mockAgent.get(REGISTRY_ORIGIN)
    pool.intercept({ path: TOKEN_EXCHANGE_PATH, method: 'POST' })
      .reply(200, { token: 'oidc-auth-token' })
    pool.intercept({ path: VISIBILITY_PATH, method: 'GET' })
      .reply(500, { code: 'BLIP', message: 'transient' })

    const result = await fetchTokenAndProvenanceByOidc(PACKAGE_NAME, REGISTRY, baseOptions)

    // The publish itself can still go through with the OIDC token — we just couldn't decide
    // whether to flip on provenance. Matches npm CLI behavior.
    expect(result).toEqual({ authToken: 'oidc-auth-token' })
    expect(result?.provenance).toBeUndefined()
    mockAgent.assertNoPendingInterceptors()
  })

  test('skips the visibility check when options.provenance is set explicitly', async () => {
    const pool = mockAgent.get(REGISTRY_ORIGIN)
    pool.intercept({ path: TOKEN_EXCHANGE_PATH, method: 'POST' })
      .reply(200, { token: 'oidc-auth-token' })

    const result = await fetchTokenAndProvenanceByOidc(
      PACKAGE_NAME,
      REGISTRY,
      { ...baseOptions, provenance: false }
    )

    expect(result).toEqual({ authToken: 'oidc-auth-token', provenance: false })
    // No visibility interceptor registered → if the orchestrator had attempted the call,
    // `disableNetConnect` would have thrown.
    mockAgent.assertNoPendingInterceptors()
  })

  test('returns undefined silently when no idToken is available — must not call the registry on local publishes', async () => {
    // The "OIDC not applicable" branch — most common case for any non-CI / un-configured
    // publish. Must not warn or hit the network. We register no interceptors; with
    // `disableNetConnect` active, any HTTP attempt would throw.
    delete process.env['NPM_ID_TOKEN']

    const result = await fetchTokenAndProvenanceByOidc(PACKAGE_NAME, REGISTRY, baseOptions)

    expect(result).toBeUndefined()
  })
})
