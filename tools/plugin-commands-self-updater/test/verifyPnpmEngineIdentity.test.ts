import crypto from 'crypto'
import fs from 'fs'
import path from 'path'

import { tempDir } from '@pnpm/prepare'
import nock from 'nock'

import { verifyPnpmEngineIdentity, type RegistryKey } from '@pnpm/tools.plugin-commands-self-updater'

const REGISTRY = 'https://registry.example.test/'
const PNPM_INTEGRITY = 'sha512-pnpm-integrity'
const EXE_INTEGRITY = 'sha512-exe-integrity'
const PLATFORM_INTEGRITY = 'sha512-platform-integrity'
const PLATFORM_PKG_NAME = '@pnpm/test-platform-arch'

beforeEach(() => {
  nock.cleanAll()
  nock.disableNetConnect()
})

afterEach(() => {
  nock.enableNetConnect()
})

test('resolves when pnpm carries a valid registry signature over the installed bytes', async () => {
  const key = createSigningKey()
  mockPackument('pnpm', PNPM_INTEGRITY, [{ keyid: key.keyid, sig: key.sign('pnpm@9.1.0', PNPM_INTEGRITY) }])
  const stage = stageWithPnpmLockfile()

  await expect(verifyPnpmEngineIdentity(stage, 'pnpm', '9.1.0', optsTrusting(key))).resolves.toBeUndefined()
})

test('throws when the installed bytes do not match what the registry signed (tamper)', async () => {
  const key = createSigningKey()
  // The registry signed the genuine integrity, but the staged lockfile pins a different one.
  mockPackument('pnpm', PNPM_INTEGRITY, [{ keyid: key.keyid, sig: key.sign('pnpm@9.1.0', 'sha512-genuine-pnpm') }])
  const stage = stageWithPnpmLockfile()

  await expect(verifyPnpmEngineIdentity(stage, 'pnpm', '9.1.0', optsTrusting(key))).rejects.toThrow(/Refusing to run pnpm/)
})

test('throws when the engine is signed by a key pnpm does not trust', async () => {
  const signingKey = createSigningKey()
  mockPackument('pnpm', PNPM_INTEGRITY, [{ keyid: signingKey.keyid, sig: signingKey.sign('pnpm@9.1.0', PNPM_INTEGRITY) }])
  const stage = stageWithPnpmLockfile()

  // Trust a different key than the one that signed.
  await expect(verifyPnpmEngineIdentity(stage, 'pnpm', '9.1.0', optsTrusting(createSigningKey()))).rejects.toThrow(/Refusing to run pnpm/)
})

test('throws when the engine version is absent from the registry', async () => {
  nock(REGISTRY).get('/pnpm').reply(404, {})
  const stage = stageWithPnpmLockfile()

  await expect(verifyPnpmEngineIdentity(stage, 'pnpm', '9.1.0', optsTrusting(createSigningKey()))).rejects.toThrow(/Refusing to run pnpm/)
})

test('throws (fails closed) when the registry is unreachable', async () => {
  // No intercept registered and net connect disabled, so the packument fetch fails.
  const stage = stageWithPnpmLockfile()

  await expect(verifyPnpmEngineIdentity(stage, 'pnpm', '9.1.0', optsTrusting(createSigningKey()))).rejects.toThrow(/Refusing to run pnpm/)
})

test('does not leak registry credentials embedded in the registry URL into error messages', async () => {
  const stage = stageWithPnpmLockfile()
  const opts = {
    ...optsTrusting(createSigningKey()),
    registries: { default: 'https://user:hunter2@registry.example.test/' },
  }

  // A non-200 response embeds the packument URL in the error.
  nock('https://registry.example.test/').get('/pnpm').reply(500, 'server error').persist()
  await expect(verifyPnpmEngineIdentity(stage, 'pnpm', '9.1.0', opts)).rejects.toThrow(/Refusing to run pnpm/)
  await expect(verifyPnpmEngineIdentity(stage, 'pnpm', '9.1.0', opts)).rejects.not.toThrow(/hunter2/)

  // An unreachable registry surfaces the fetch-layer error, which embeds the
  // request URL.
  nock.cleanAll()
  await expect(verifyPnpmEngineIdentity(stage, 'pnpm', '9.1.0', opts)).rejects.toThrow(/Refusing to run pnpm/)
  await expect(verifyPnpmEngineIdentity(stage, 'pnpm', '9.1.0', opts)).rejects.not.toThrow(/hunter2/)
})

test('skips (no throw) when no trusted keys are provided', async () => {
  const stage = tempDir(false)

  await expect(verifyPnpmEngineIdentity(stage, 'pnpm', '9.1.0', { ...baseOpts(), trustedKeys: [] })).resolves.toBeUndefined()
})

test('throws when the engine component in the staged lockfile has no integrity metadata', async () => {
  const key = createSigningKey()
  mockPackument('pnpm', PNPM_INTEGRITY, [{ keyid: key.keyid, sig: key.sign('pnpm@9.1.0', PNPM_INTEGRITY) }])
  const stage = tempDir(false)
  writeStageLockfile(stage, [
    'lockfileVersion: \'9.0\'',
    'importers:',
    '  .:',
    '    dependencies:',
    '      pnpm:',
    '        specifier: 9.1.0',
    '        version: 9.1.0',
    'packages:',
    '  pnpm@9.1.0:',
    `    resolution: {tarball: ${REGISTRY}pnpm/-/pnpm-9.1.0.tgz}`,
    'snapshots:',
    '  pnpm@9.1.0: {}',
  ])

  await expect(verifyPnpmEngineIdentity(stage, 'pnpm', '9.1.0', optsTrusting(key))).rejects.toThrow(/integrity metadata is missing/)
})

test('verifies the platform binary materialized for an @pnpm/exe install', async () => {
  const key = createSigningKey()
  mockPackument('@pnpm/exe', EXE_INTEGRITY, [{ keyid: key.keyid, sig: key.sign('@pnpm/exe@9.1.0', EXE_INTEGRITY) }])
  mockPackument(PLATFORM_PKG_NAME, PLATFORM_INTEGRITY, [{ keyid: key.keyid, sig: key.sign(`${PLATFORM_PKG_NAME}@9.1.0`, PLATFORM_INTEGRITY) }])
  const stage = stageWithExeLockfile(PLATFORM_INTEGRITY)

  await expect(verifyPnpmEngineIdentity(stage, '@pnpm/exe', '9.1.0', optsTrusting(key))).resolves.toBeUndefined()
})

test('throws when the platform binary signature does not validate over the installed bytes', async () => {
  const key = createSigningKey()
  mockPackument('@pnpm/exe', EXE_INTEGRITY, [{ keyid: key.keyid, sig: key.sign('@pnpm/exe@9.1.0', EXE_INTEGRITY) }])
  // The registry signed a different (genuine) platform binary than the one staged.
  mockPackument(PLATFORM_PKG_NAME, PLATFORM_INTEGRITY, [{ keyid: key.keyid, sig: key.sign(`${PLATFORM_PKG_NAME}@9.1.0`, 'sha512-genuine-platform') }])
  const stage = stageWithExeLockfile(PLATFORM_INTEGRITY)

  await expect(verifyPnpmEngineIdentity(stage, '@pnpm/exe', '9.1.0', optsTrusting(key))).rejects.toThrow(/Refusing to run pnpm/)
})

test('verifies the platform binary materialized for a pnpm v12 install (the pnpm package is itself native)', async () => {
  const key = createSigningKey()
  mockPackument('pnpm', PNPM_INTEGRITY, [{ keyid: key.keyid, sig: key.sign('pnpm@12.0.0', PNPM_INTEGRITY) }], '12.0.0')
  mockPackument(PLATFORM_PKG_NAME, PLATFORM_INTEGRITY, [{ keyid: key.keyid, sig: key.sign(`${PLATFORM_PKG_NAME}@12.0.0`, PLATFORM_INTEGRITY) }], '12.0.0')
  const stage = stageWithPnpmV12Lockfile(PLATFORM_INTEGRITY)

  await expect(verifyPnpmEngineIdentity(stage, 'pnpm', '12.0.0', optsTrusting(key))).resolves.toBeUndefined()
})

test('throws when the pnpm v12 platform binary signature does not validate over the installed bytes', async () => {
  const key = createSigningKey()
  mockPackument('pnpm', PNPM_INTEGRITY, [{ keyid: key.keyid, sig: key.sign('pnpm@12.0.0', PNPM_INTEGRITY) }], '12.0.0')
  // The registry signed a different (genuine) platform binary than the one staged.
  mockPackument(PLATFORM_PKG_NAME, PLATFORM_INTEGRITY, [{ keyid: key.keyid, sig: key.sign(`${PLATFORM_PKG_NAME}@12.0.0`, 'sha512-genuine-platform') }], '12.0.0')
  const stage = stageWithPnpmV12Lockfile(PLATFORM_INTEGRITY)

  await expect(verifyPnpmEngineIdentity(stage, 'pnpm', '12.0.0', optsTrusting(key))).rejects.toThrow(/Refusing to run pnpm/)
})

function baseOpts () {
  return {
    rawConfig: {},
    registries: { default: REGISTRY },
    retry: { retries: 0 },
  }
}

function optsTrusting (key: ReturnType<typeof createSigningKey>) {
  const trustedKey: RegistryKey = {
    expires: null,
    key: key.publicKey,
    keyid: key.keyid,
    keytype: 'ecdsa-sha2-nistp256',
    scheme: 'ecdsa-sha2-nistp256',
  }
  return { ...baseOpts(), trustedKeys: [trustedKey] }
}

function stageWithPnpmLockfile (): string {
  const stage = tempDir(false)
  writeStageLockfile(stage, [
    'lockfileVersion: \'9.0\'',
    'importers:',
    '  .:',
    '    dependencies:',
    '      pnpm:',
    '        specifier: 9.1.0',
    '        version: 9.1.0',
    'packages:',
    '  pnpm@9.1.0:',
    `    resolution: {integrity: ${PNPM_INTEGRITY}}`,
    'snapshots:',
    '  pnpm@9.1.0: {}',
  ])
  return stage
}

function stageWithExeLockfile (platformIntegrity: string): string {
  const stage = tempDir(false)
  writeStageLockfile(stage, [
    'lockfileVersion: \'9.0\'',
    'importers:',
    '  .:',
    '    dependencies:',
    '      \'@pnpm/exe\':',
    '        specifier: 9.1.0',
    '        version: 9.1.0',
    'packages:',
    '  \'@pnpm/exe@9.1.0\':',
    `    resolution: {integrity: ${EXE_INTEGRITY}}`,
    `  '${PLATFORM_PKG_NAME}@9.1.0':`,
    `    resolution: {integrity: ${platformIntegrity}}`,
    'snapshots:',
    '  \'@pnpm/exe@9.1.0\':',
    '    optionalDependencies:',
    `      '${PLATFORM_PKG_NAME}': 9.1.0`,
    `  '${PLATFORM_PKG_NAME}@9.1.0':`,
    '    optional: true',
  ])
  // Only platform packages actually materialized on disk are verified.
  fs.mkdirSync(path.join(stage, 'node_modules', PLATFORM_PKG_NAME), { recursive: true })
  return stage
}

function stageWithPnpmV12Lockfile (platformIntegrity: string): string {
  const stage = tempDir(false)
  writeStageLockfile(stage, [
    'lockfileVersion: \'9.0\'',
    'importers:',
    '  .:',
    '    dependencies:',
    '      pnpm:',
    '        specifier: 12.0.0',
    '        version: 12.0.0',
    'packages:',
    '  pnpm@12.0.0:',
    `    resolution: {integrity: ${PNPM_INTEGRITY}}`,
    `  '${PLATFORM_PKG_NAME}@12.0.0':`,
    `    resolution: {integrity: ${platformIntegrity}}`,
    'snapshots:',
    '  pnpm@12.0.0:',
    '    optionalDependencies:',
    `      '${PLATFORM_PKG_NAME}': 12.0.0`,
    `  '${PLATFORM_PKG_NAME}@12.0.0':`,
    '    optional: true',
  ])
  // Only platform packages actually materialized on disk are verified.
  fs.mkdirSync(path.join(stage, 'node_modules', PLATFORM_PKG_NAME), { recursive: true })
  return stage
}

function writeStageLockfile (stage: string, lines: string[]): void {
  fs.writeFileSync(path.join(stage, 'pnpm-lock.yaml'), `${lines.join('\n')}\n`, 'utf8')
}

function mockPackument (name: string, integrity: string, signatures: unknown, version = '9.1.0'): void {
  const encodedPath = name[0] === '@' ? `/@${encodeURIComponent(name.slice(1))}` : `/${name}`
  nock(REGISTRY)
    .get(encodedPath)
    .reply(200, {
      name,
      time: { [version]: '2024-01-01T00:00:00.000Z' },
      versions: {
        [version]: { name, version, dist: { integrity, signatures, tarball: `${REGISTRY}${name}/-/x-${version}.tgz` } },
      },
    })
    .persist()
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
