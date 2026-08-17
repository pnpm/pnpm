import crypto from 'node:crypto'

import { afterEach, beforeEach, describe, expect, test } from '@jest/globals'
import type { EnvLockfile } from '@pnpm/lockfile.types'
import { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'
import { familySync } from 'detect-libc'

const { exePlatformPkgDirName, exePlatformPkgDirNameNext, verifyPnpmEngineIdentity } = await import('@pnpm/engine.pm.commands')

const REGISTRY = 'https://registry.example.test/'
const CANONICAL_REGISTRY = 'https://registry.npmjs.org/'
const PNPM_INTEGRITY = 'sha512-pnpm-integrity'
const EXE_INTEGRITY = 'sha512-exe-integrity'
const PLATFORM_INTEGRITY = 'sha512-platform-integrity'
const PLATFORM_PKG_NAME = `@pnpm/${exePlatformPkgDirName(process.platform, process.arch, familySync())}`
const PLATFORM_PKG_NAME_NEXT = `@pnpm/${exePlatformPkgDirNameNext(process.platform, process.arch, familySync())}`

beforeEach(async () => {
  await setupMockAgent()
})

afterEach(async () => {
  await teardownMockAgent()
})

describe('verifyPnpmEngineIdentity', () => {
  test('resolves when both pnpm and @pnpm/exe carry a valid registry signature over the installed bytes', async () => {
    const key = createSigningKey()
    mockPackument({ name: 'pnpm', integrity: PNPM_INTEGRITY, signatures: [{ keyid: key.keyid, sig: key.sign('pnpm@9.1.0', PNPM_INTEGRITY) }] })
    mockPackument({ name: '@pnpm/exe', integrity: EXE_INTEGRITY, signatures: [{ keyid: key.keyid, sig: key.sign('@pnpm/exe@9.1.0', EXE_INTEGRITY) }] })
    mockPackument({ name: PLATFORM_PKG_NAME, integrity: PLATFORM_INTEGRITY, signatures: [{ keyid: key.keyid, sig: key.sign(`${PLATFORM_PKG_NAME}@9.1.0`, PLATFORM_INTEGRITY) }] })

    await expect(verifyPnpmEngineIdentity(envLockfile(), '9.1.0', optsTrusting(key))).resolves.toBeUndefined()
  })

  test('throws when the installed bytes do not match what the registry signed (tamper)', async () => {
    const key = createSigningKey()
    // The registry signed the genuine integrity, but the lockfile pins a different one.
    mockPackument({ name: 'pnpm', integrity: PNPM_INTEGRITY, signatures: [{ keyid: key.keyid, sig: key.sign('pnpm@9.1.0', 'sha512-genuine-pnpm') }] })
    mockPackument({ name: '@pnpm/exe', integrity: EXE_INTEGRITY, signatures: [{ keyid: key.keyid, sig: key.sign('@pnpm/exe@9.1.0', 'sha512-genuine-exe') }] })

    await expect(verifyPnpmEngineIdentity(envLockfile(), '9.1.0', optsTrusting(key))).rejects.toThrow(/Refusing to run pnpm/)
  })

  test('throws when the engine is signed by a key pnpm does not trust', async () => {
    const signingKey = createSigningKey()
    mockPackument({ name: 'pnpm', integrity: PNPM_INTEGRITY, signatures: [{ keyid: signingKey.keyid, sig: signingKey.sign('pnpm@9.1.0', PNPM_INTEGRITY) }] })
    mockPackument({ name: '@pnpm/exe', integrity: EXE_INTEGRITY, signatures: [{ keyid: signingKey.keyid, sig: signingKey.sign('@pnpm/exe@9.1.0', EXE_INTEGRITY) }] })

    // Trust a different key than the one that signed.
    await expect(verifyPnpmEngineIdentity(envLockfile(), '9.1.0', optsTrusting(createSigningKey()))).rejects.toThrow(/Refusing to run pnpm/)
  })

  test('throws when the engine version is absent from both the registry and the canonical registry', async () => {
    for (const registry of [REGISTRY, CANONICAL_REGISTRY]) {
      getMockAgent().get(registry.replace(/\/$/, ''))
        .intercept({ path: '/pnpm', method: 'GET' }).reply(404, {})
      getMockAgent().get(registry.replace(/\/$/, ''))
        .intercept({ path: '/@pnpm%2Fexe', method: 'GET' }).reply(404, {}) // cspell:disable-line
    }

    await expect(verifyPnpmEngineIdentity(envLockfile(), '9.1.0', optsTrusting(createSigningKey()))).rejects.toThrow(/Refusing to run pnpm/)
  })

  test('throws (fails closed) when the canonical registry is the configured registry and is unreachable', async () => {
    // No intercept registered and net connect disabled, so the packument fetch fails.
    const opts = { ...optsTrusting(createSigningKey()), registriesByScope: { default: CANONICAL_REGISTRY } }
    await expect(verifyPnpmEngineIdentity(envLockfile(), '9.1.0', opts)).rejects.toThrow(/Refusing to run pnpm/)
  })

  test('recognizes URL-equivalent spellings of the canonical registry and still fails closed', async () => {
    // Case, an explicit default port, and inline credentials are all
    // URL-equivalent to the canonical registry and must not unlock the
    // warn-and-proceed path.
    await Promise.all(['https://Registry.NPMJS.org:443/', 'https://user:pass@registry.npmjs.org/'].map(async (canonical) => {
      const opts = { ...optsTrusting(createSigningKey()), registriesByScope: { default: canonical } }
      await expect(verifyPnpmEngineIdentity(envLockfile(), '9.1.0', opts)).rejects.toThrow(/Refusing to run pnpm/)
    }))
  })

  test('warns and proceeds when a user-configured registry is unreachable and no signature is obtainable', async () => {
    // No intercept registered and net connect disabled: neither the configured
    // registry nor the canonical fallback can provide a signature. The
    // configured registry comes from trusted (non-project) configuration, so
    // the switch proceeds on the lockfile integrity pin alone.
    await expect(verifyPnpmEngineIdentity(envLockfile(), '9.1.0', optsTrusting(createSigningKey()))).resolves.toBeUndefined()
  })

  test('verifies via the canonical registry when the configured registry serves no signatures', async () => {
    const key = createSigningKey()
    mockPackument({ name: 'pnpm', integrity: PNPM_INTEGRITY, signatures: [] })
    mockPackument({ name: '@pnpm/exe', integrity: EXE_INTEGRITY, signatures: [] })
    mockPackument({ name: PLATFORM_PKG_NAME, integrity: PLATFORM_INTEGRITY, signatures: [] })
    mockPackument({ name: 'pnpm', integrity: PNPM_INTEGRITY, signatures: [{ keyid: key.keyid, sig: key.sign('pnpm@9.1.0', PNPM_INTEGRITY) }], registry: CANONICAL_REGISTRY })
    mockPackument({ name: '@pnpm/exe', integrity: EXE_INTEGRITY, signatures: [{ keyid: key.keyid, sig: key.sign('@pnpm/exe@9.1.0', EXE_INTEGRITY) }], registry: CANONICAL_REGISTRY })
    mockPackument({ name: PLATFORM_PKG_NAME, integrity: PLATFORM_INTEGRITY, signatures: [{ keyid: key.keyid, sig: key.sign(`${PLATFORM_PKG_NAME}@9.1.0`, PLATFORM_INTEGRITY) }], registry: CANONICAL_REGISTRY })

    await expect(verifyPnpmEngineIdentity(envLockfile(), '9.1.0', optsTrusting(key))).resolves.toBeUndefined()
  })

  test('a canonical-registry signature still catches a tampered integrity when the configured registry serves no signatures', async () => {
    const key = createSigningKey()
    mockPackument({ name: 'pnpm', integrity: PNPM_INTEGRITY, signatures: [] })
    mockPackument({ name: '@pnpm/exe', integrity: EXE_INTEGRITY, signatures: [] })
    mockPackument({ name: PLATFORM_PKG_NAME, integrity: PLATFORM_INTEGRITY, signatures: [] })
    // The canonical registry signed different bytes than the lockfile pins.
    mockPackument({ name: 'pnpm', integrity: PNPM_INTEGRITY, signatures: [{ keyid: key.keyid, sig: key.sign('pnpm@9.1.0', 'sha512-genuine-pnpm') }], registry: CANONICAL_REGISTRY })
    mockPackument({ name: '@pnpm/exe', integrity: EXE_INTEGRITY, signatures: [{ keyid: key.keyid, sig: key.sign('@pnpm/exe@9.1.0', 'sha512-genuine-exe') }], registry: CANONICAL_REGISTRY })
    mockPackument({ name: PLATFORM_PKG_NAME, integrity: PLATFORM_INTEGRITY, signatures: [{ keyid: key.keyid, sig: key.sign(`${PLATFORM_PKG_NAME}@9.1.0`, 'sha512-genuine-platform') }], registry: CANONICAL_REGISTRY })

    await expect(verifyPnpmEngineIdentity(envLockfile(), '9.1.0', optsTrusting(key))).rejects.toThrow(/Refusing to run pnpm/)
  })

  test('warns and proceeds when a user-configured registry pins a non-sha512 integrity no signature can cover', async () => {
    // A registry that publishes only `shasum` yields sha1 pins; no npm
    // signature can ever validate over them, so no packument is even fetched.
    const lockfile = envLockfile()
    ;(lockfile.packages as Record<string, { resolution: { integrity: string } }>)['pnpm@9.1.0'] = { resolution: { integrity: 'sha1-8bee00286a17c00a13c7e6e6dd9a9b389220ee7f' } }
    ;(lockfile.packages as Record<string, { resolution: { integrity: string } }>)['@pnpm/exe@9.1.0'] = { resolution: { integrity: 'sha1-9bee00286a17c00a13c7e6e6dd9a9b389220ee7f' } }
    ;(lockfile.packages as Record<string, { resolution: { integrity: string } }>)[`${PLATFORM_PKG_NAME}@9.1.0`] = { resolution: { integrity: 'sha1-abee00286a17c00a13c7e6e6dd9a9b389220ee7f' } }

    await expect(verifyPnpmEngineIdentity(lockfile, '9.1.0', optsTrusting(createSigningKey()))).resolves.toBeUndefined()
  })

  test('throws for a non-sha512 integrity pin when the engine resolves from the canonical registry', async () => {
    const lockfile = envLockfile()
    ;(lockfile.packages as Record<string, { resolution: { integrity: string } }>)['pnpm@9.1.0'] = { resolution: { integrity: 'sha1-8bee00286a17c00a13c7e6e6dd9a9b389220ee7f' } }

    const opts = { ...optsTrusting(createSigningKey()), registriesByScope: { default: CANONICAL_REGISTRY } }
    await expect(verifyPnpmEngineIdentity(lockfile, '9.1.0', opts)).rejects.toThrow(/Refusing to run pnpm/)
  })

  test('skips (no throw) when no trusted keys are provided', async () => {
    await expect(verifyPnpmEngineIdentity(envLockfile(), '9.1.0', { registriesByScope: { default: REGISTRY }, trustedKeys: [] })).resolves.toBeUndefined()
  })

  test('throws when an engine component in the lockfile has no integrity metadata', async () => {
    const key = createSigningKey()
    mockPackument({ name: 'pnpm', integrity: PNPM_INTEGRITY, signatures: [{ keyid: key.keyid, sig: key.sign('pnpm@9.1.0', PNPM_INTEGRITY) }] })

    const lockfile = envLockfile()
    ;(lockfile.packages as Record<string, unknown>)['@pnpm/exe@9.1.0'] = { resolution: { tarball: `${REGISTRY}@pnpm/exe/-/exe-9.1.0.tgz` } }

    await expect(verifyPnpmEngineIdentity(lockfile, '9.1.0', optsTrusting(key))).rejects.toThrow(/integrity metadata is missing/)
  })

  test('throws when the platform binary in the lockfile has no integrity metadata', async () => {
    const key = createSigningKey()
    mockPackument({ name: 'pnpm', integrity: PNPM_INTEGRITY, signatures: [{ keyid: key.keyid, sig: key.sign('pnpm@9.1.0', PNPM_INTEGRITY) }] })
    mockPackument({ name: '@pnpm/exe', integrity: EXE_INTEGRITY, signatures: [{ keyid: key.keyid, sig: key.sign('@pnpm/exe@9.1.0', EXE_INTEGRITY) }] })

    const lockfile = envLockfile()
    ;(lockfile.snapshots as Record<string, unknown>)['@pnpm/exe@9.1.0'] = { optionalDependencies: { [PLATFORM_PKG_NAME]: '9.1.0' } }
    ;(lockfile.packages as Record<string, unknown>)[`${PLATFORM_PKG_NAME}@9.1.0`] = { resolution: { tarball: `${REGISTRY}${PLATFORM_PKG_NAME}/-/x-9.1.0.tgz` } }

    await expect(verifyPnpmEngineIdentity(lockfile, '9.1.0', optsTrusting(key))).rejects.toThrow(/integrity metadata is missing/)
  })

  test('resolves when the platform binary carries a valid registry signature', async () => {
    const key = createSigningKey()
    mockPackument({ name: 'pnpm', integrity: PNPM_INTEGRITY, signatures: [{ keyid: key.keyid, sig: key.sign('pnpm@9.1.0', PNPM_INTEGRITY) }] })
    mockPackument({ name: '@pnpm/exe', integrity: EXE_INTEGRITY, signatures: [{ keyid: key.keyid, sig: key.sign('@pnpm/exe@9.1.0', EXE_INTEGRITY) }] })
    mockPackument({ name: PLATFORM_PKG_NAME, integrity: PLATFORM_INTEGRITY, signatures: [{ keyid: key.keyid, sig: key.sign(`${PLATFORM_PKG_NAME}@9.1.0`, PLATFORM_INTEGRITY) }] })

    const lockfile = envLockfile()
    ;(lockfile.snapshots as Record<string, unknown>)['@pnpm/exe@9.1.0'] = { optionalDependencies: { [PLATFORM_PKG_NAME]: '9.1.0' } }
    ;(lockfile.packages as Record<string, unknown>)[`${PLATFORM_PKG_NAME}@9.1.0`] = { resolution: { integrity: PLATFORM_INTEGRITY } }

    await expect(verifyPnpmEngineIdentity(lockfile, '9.1.0', optsTrusting(key))).resolves.toBeUndefined()
  })

  test('throws when the wrapper snapshot lists no platform binaries', async () => {
    const key = createSigningKey()
    const lockfile = envLockfile()
    ;(lockfile.snapshots as Record<string, unknown>)['@pnpm/exe@9.1.0'] = {}

    await expect(verifyPnpmEngineIdentity(lockfile, '9.1.0', optsTrusting(key))).rejects.toThrow(/platform binaries are missing/)
  })

  test('throws when no platform binary in the lockfile matches the host', async () => {
    const key = createSigningKey()
    const lockfile = envLockfile()
    ;(lockfile.snapshots as Record<string, unknown>)['@pnpm/exe@9.1.0'] = { optionalDependencies: { '@pnpm/exe.aix-mips': '9.1.0' } }

    await expect(verifyPnpmEngineIdentity(lockfile, '9.1.0', optsTrusting(key))).rejects.toThrow(/native binary: it is missing/)
  })

  test('verifies the platform binary of the unscoped pnpm, which is the native wrapper from v12', async () => {
    const key = createSigningKey()
    mockPackument({ name: 'pnpm', integrity: PNPM_INTEGRITY, signatures: [{ keyid: key.keyid, sig: key.sign('pnpm@12.0.0', PNPM_INTEGRITY) }], version: '12.0.0' })
    mockPackument({ name: PLATFORM_PKG_NAME_NEXT, integrity: PLATFORM_INTEGRITY, signatures: [{ keyid: key.keyid, sig: key.sign(`${PLATFORM_PKG_NAME_NEXT}@12.0.0`, 'sha512-genuine-platform') }], version: '12.0.0' })

    await expect(verifyPnpmEngineIdentity(envLockfileV12(), '12.0.0', optsTrusting(key))).rejects.toThrow(/Refusing to run pnpm/)
  })
})

/** An env lockfile of a v12 engine, whose only package-manager dependency is the native `pnpm`. */
function envLockfileV12 (): EnvLockfile {
  return {
    lockfileVersion: '9.0',
    importers: {
      '.': {
        configDependencies: {},
        packageManagerDependencies: {
          pnpm: { specifier: '12.0.0', version: '12.0.0' },
        },
      },
    },
    packages: {
      'pnpm@12.0.0': { resolution: { integrity: PNPM_INTEGRITY } },
      [`${PLATFORM_PKG_NAME_NEXT}@12.0.0`]: { resolution: { integrity: PLATFORM_INTEGRITY } },
    },
    snapshots: {
      'pnpm@12.0.0': { optionalDependencies: { [PLATFORM_PKG_NAME_NEXT]: '12.0.0' } },
    },
  } as unknown as EnvLockfile
}

function optsTrusting (key: ReturnType<typeof createSigningKey>) {
  return {
    registriesByScope: { default: REGISTRY },
    trustedKeys: [{ expires: null, key: key.publicKey, keyid: key.keyid, keytype: 'ecdsa-sha2-nistp256', scheme: 'ecdsa-sha2-nistp256' }],
  }
}

function envLockfile (): EnvLockfile {
  return {
    lockfileVersion: '9.0',
    importers: {
      '.': {
        configDependencies: {},
        packageManagerDependencies: {
          pnpm: { specifier: '9.1.0', version: '9.1.0' },
          '@pnpm/exe': { specifier: '9.1.0', version: '9.1.0' },
        },
      },
    },
    packages: {
      'pnpm@9.1.0': { resolution: { integrity: PNPM_INTEGRITY } },
      '@pnpm/exe@9.1.0': { resolution: { integrity: EXE_INTEGRITY } },
      [`${PLATFORM_PKG_NAME}@9.1.0`]: { resolution: { integrity: PLATFORM_INTEGRITY } },
    },
    snapshots: {
      'pnpm@9.1.0': {},
      '@pnpm/exe@9.1.0': { optionalDependencies: { [PLATFORM_PKG_NAME]: '9.1.0' } },
    },
  } as unknown as EnvLockfile
}

function mockPackument ({ name, integrity, signatures, version = '9.1.0', registry = REGISTRY }: { name: string, integrity: string, signatures: unknown, version?: string, registry?: string }): void {
  const encodedPath = name[0] === '@' ? `/${name.replace(/\//g, '%2F')}` : `/${name}`
  getMockAgent().get(registry.replace(/\/$/, ''))
    .intercept({ path: encodedPath, method: 'GET' })
    .reply(200, {
      name,
      time: { [version]: '2024-01-01T00:00:00.000Z' },
      versions: {
        [version]: { name, version, dist: { integrity, signatures, tarball: `${REGISTRY}${name}/-/x-${version}.tgz` } },
      },
    }).persist()
}

function createSigningKey (): { keyid: string, publicKey: string, sign: (id: string, integrity: string) => string } {
  const { privateKey, publicKey } = crypto.generateKeyPairSync('ec', { namedCurve: 'prime256v1' })
  const publicKeyPem = publicKey.export({ format: 'pem', type: 'spki' }).toString()
  return {
    keyid: `SHA256:test-key-${crypto.randomBytes(4).toString('hex')}`,
    publicKey: publicKeyPem.replace(/-----BEGIN PUBLIC KEY-----|-----END PUBLIC KEY-----|\s/g, ''),
    sign: (id, integrity) => {
      const signer = crypto.createSign('SHA256')
      signer.write(`${id}:${integrity}`)
      signer.end()
      return signer.sign(privateKey, 'base64')
    },
  }
}
