import assert from 'node:assert/strict'
import { createSign, generateKeyPairSync } from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { after, before, describe, test } from 'node:test'

import { parseCliArgs, setupCommand } from '../lib/install.js'
import { platformPackageName, type Target } from '../lib/platformPackageName.js'
import { resolveVersion } from '../lib/resolveVersion.js'
import { sameFileContents } from '../lib/sameFileContents.js'
import { renderUndoHint } from '../lib/undoHint.js'
import { type SigningKey, verifyRegistrySignature } from '../lib/verifySignature.js'

describe('resolveVersion', () => {
  const packument = {
    'dist-tags': { latest: '11.20.0', 'latest-10': '10.34.5', 'next-12': '12.0.0-rc.1' },
    versions: { '10.34.5': {}, '11.19.0': {}, '11.20.0': {}, '12.0.0-rc.1': {} },
  }

  test('resolves a dist-tag', () => {
    assert.equal(resolveVersion(packument, 'latest'), '11.20.0')
    assert.equal(resolveVersion(packument, 'next-12'), '12.0.0-rc.1')
  })

  test('resolves an exact version, with or without a leading v', () => {
    assert.equal(resolveVersion(packument, '11.19.0'), '11.19.0')
    assert.equal(resolveVersion(packument, 'v11.19.0'), '11.19.0')
  })

  test('resolves a bare major, preferring the stable release', () => {
    assert.equal(resolveVersion(packument, '10'), '10.34.5')
    assert.equal(resolveVersion(packument, '12'), '12.0.0-rc.1')
  })

  test('resolves a range to the highest version in it', () => {
    assert.equal(resolveVersion(packument, '^10.0.0'), '10.34.5')
    assert.equal(resolveVersion(packument, '~11.19.0'), '11.19.0')
    assert.equal(resolveVersion(packument, '>=10 <11.20'), '11.19.0')
    assert.equal(resolveVersion(packument, '10.x || 11.x'), '11.20.0')
  })

  test('resolves a range to latest when latest is in it', () => {
    const backported = { ...packument, versions: { ...packument.versions, '11.21.0': {} } }
    assert.equal(resolveVersion(backported, '^11.0.0'), '11.20.0')
    assert.equal(resolveVersion(backported, '^11.20.1'), '11.21.0')
  })

  test('resolves a range to a prerelease only when the range names one', () => {
    assert.equal(resolveVersion(packument, '>=11'), '11.20.0')
    assert.equal(resolveVersion(packument, '^12.0.0-0'), '12.0.0-rc.1')
    assert.throws(() => resolveVersion(packument, '^12.0.0'), /could not be found/)
  })

  test('lists the available tags when nothing matches', () => {
    assert.throws(() => resolveVersion(packument, '9'), /could not be found.*latest, latest-10, next-12/s)
    assert.throws(() => resolveVersion(packument, '^9.0.0'), /could not be found/)
    assert.throws(() => resolveVersion(packument, 'not-a-tag'), /could not be found/)
  })
})

describe('platformPackageName', () => {
  const target = (overrides: Partial<Target>): Target =>
    ({ major: 11, platform: 'linux', arch: 'x64', musl: false, ...overrides })

  test('v11 and older use the legacy names', () => {
    assert.equal(platformPackageName(target({})), '@pnpm/linux-x64')
    assert.equal(platformPackageName(target({ musl: true })), '@pnpm/linuxstatic-x64')
    assert.equal(platformPackageName(target({ platform: 'darwin', arch: 'arm64' })), '@pnpm/macos-arm64')
    assert.equal(platformPackageName(target({ platform: 'win32' })), '@pnpm/win-x64')
  })

  test('v12 and newer use the process.platform-based names', () => {
    assert.equal(platformPackageName(target({ major: 12 })), '@pnpm/exe.linux-x64')
    assert.equal(platformPackageName(target({ major: 12, musl: true })), '@pnpm/exe.linux-x64-musl')
    assert.equal(platformPackageName(target({ major: 12, platform: 'win32', arch: 'arm64' })), '@pnpm/exe.win32-arm64')
  })

  test('the musl suffix is Linux-only', () => {
    assert.equal(platformPackageName(target({ platform: 'darwin', arch: 'arm64', musl: true })), '@pnpm/macos-arm64')
  })

  test('v12 builds for hosts outside the x64/arm64 matrix', () => {
    assert.equal(platformPackageName(target({ major: 12, platform: 'freebsd' })), '@pnpm/exe.freebsd-x64')
    assert.equal(platformPackageName(target({ major: 12, arch: 'ppc64' })), '@pnpm/exe.linux-ppc64')
    assert.equal(platformPackageName(target({ major: 12, arch: 'riscv64' })), '@pnpm/exe.linux-riscv64')
    assert.equal(platformPackageName(target({ major: 12, arch: 's390x' })), '@pnpm/exe.linux-s390x')
    assert.equal(platformPackageName(target({ major: 12, platform: 'android', arch: 'arm64' })), '@pnpm/exe.android-arm64')
    assert.equal(platformPackageName(target({ major: 12, platform: 'android' })), '@pnpm/exe.android-x64')
  })

  test('those hosts have no binary before v12, and are pointed at it', () => {
    assert.throws(() => platformPackageName(target({ platform: 'freebsd' })), /pnpm v11 does not.*npx get-pnpm 12/s)
    assert.throws(() => platformPackageName(target({ arch: 's390x' })), /npx get-pnpm 12/)
  })

  test('a musl suffix is never added to them', () => {
    assert.equal(platformPackageName(target({ major: 12, arch: 'ppc64', musl: true })), '@pnpm/exe.linux-ppc64')
    // Android is bionic. `isMusl()` only ever reports true on Linux, but the
    // name must not carry a libc either way.
    assert.equal(platformPackageName(target({ major: 12, platform: 'android', musl: true })), '@pnpm/exe.android-x64')
  })

  test('rejects hosts pnpm publishes no binary for', () => {
    assert.throws(() => platformPackageName(target({ arch: 'ia32' })), /linux-ia32/)
    assert.throws(() => platformPackageName(target({ major: 12, arch: 'ia32' })), /linux-ia32/)
    assert.throws(() => platformPackageName(target({ major: 12, platform: 'openbsd' })), /openbsd-x64/)
    // riscv64 is built for Linux only, so it is not a free pass on any OS.
    assert.throws(() => platformPackageName(target({ major: 12, platform: 'openbsd', arch: 'riscv64' })), /openbsd-riscv64/)
  })

  test('points Intel macOS users away from v11, which has no working binary', () => {
    assert.throws(() => platformPackageName(target({ platform: 'darwin', arch: 'x64' })), /Intel macOS/)
    assert.equal(platformPackageName(target({ major: 10, platform: 'darwin', arch: 'x64' })), '@pnpm/macos-x64')
  })
})

describe('verifyRegistrySignature', () => {
  const { privateKey, publicKey } = generateKeyPairSync('ec', { namedCurve: 'prime256v1' })
  const KEY_ID = 'SHA256:test-key'
  const keys: SigningKey[] = [{
    keyid: KEY_ID,
    key: publicKey.export({ type: 'spki', format: 'der' }).toString('base64'),
    expires: null,
  }]
  const name = '@pnpm/exe.linux-x64'
  const version = '12.0.0'
  const integrity = 'sha512-Zm9vYmFy'
  const sign = (message: string, key = privateKey): string => createSign('SHA256').update(message).sign(key, 'base64')
  const signatures = [{ keyid: KEY_ID, sig: sign(`${name}@${version}:${integrity}`) }]

  test('accepts a signature over the package identity and checksum', () => {
    verifyRegistrySignature({ name, version, integrity, signatures, keys })
  })

  test('rejects a signature made over a different checksum', () => {
    assert.throws(() => verifyRegistrySignature({
      name, version, integrity, keys,
      signatures: [{ keyid: KEY_ID, sig: sign(`${name}@${version}:sha512-dGFtcGVyZWQ=`) }],
    }), /is not valid/)
  })

  test('rejects a signature made over a different package', () => {
    assert.throws(() => verifyRegistrySignature({
      name, version, integrity, keys,
      signatures: [{ keyid: KEY_ID, sig: sign(`@evil/pkg@${version}:${integrity}`) }],
    }), /is not valid/)
  })

  test('rejects a signature made with an untrusted key', () => {
    const attacker = generateKeyPairSync('ec', { namedCurve: 'prime256v1' })
    assert.throws(() => verifyRegistrySignature({
      name, version, integrity, keys,
      signatures: [{ keyid: KEY_ID, sig: sign(`${name}@${version}:${integrity}`, attacker.privateKey) }],
    }), /is not valid/)
  })

  test('rejects a keyid that is not pinned', () => {
    assert.throws(() => verifyRegistrySignature({
      name, version, integrity, keys,
      signatures: [{ keyid: 'SHA256:someone-else', sig: signatures[0]!.sig }],
    }), /unexpected npm key/)
  })

  test('rejects an unsigned package', () => {
    assert.throws(() => verifyRegistrySignature({ name, version, integrity, keys }), /carries no npm registry signature/)
    assert.throws(() => verifyRegistrySignature({ name, version, integrity, keys, signatures: [] }), /carries no npm registry signature/)
  })

  test('picks the signature made with a pinned key, whatever its position', () => {
    const rotated = [
      { keyid: 'SHA256:not-pinned-yet', sig: sign(`${name}@${version}:${integrity}`) },
      ...signatures,
    ]
    verifyRegistrySignature({ name, version, integrity, signatures: rotated, keys })
  })

  test('rejects a key whose expiry date cannot be read', () => {
    assert.throws(() => verifyRegistrySignature({
      name, version, integrity, signatures, keys: [{ ...keys[0]!, expires: 'not a date' }],
    }), /expiry date cannot be read/)
  })

  test('rejects a key that has expired, and accepts one that has not', () => {
    const expiring = [{ ...keys[0]!, expires: '2020-01-01T00:00:00.000Z' }]
    assert.throws(() => verifyRegistrySignature({ name, version, integrity, signatures, keys: expiring }), /expired on 2020-01-01/)
    verifyRegistrySignature({ name, version, integrity, signatures, keys: expiring, now: new Date('2019-01-01') })
  })
})

describe('sameFileContents', () => {
  let dir: string

  before(() => {
    dir = fs.mkdtempSync(path.join(os.tmpdir(), 'get-pnpm-same-'))
  })
  after(() => {
    fs.rmSync(dir, { recursive: true, force: true })
  })

  const write = (name: string, content: string | Buffer): string => {
    const file = path.join(dir, name)
    fs.writeFileSync(file, content)
    return file
  }

  test('accepts two files holding the same bytes', () => {
    // Longer than one read, so the comparison has to loop.
    const content = Buffer.alloc(200_000, 'pnpm')
    assert.equal(sameFileContents(write('a', content), write('b', content)), true)
  })

  test('rejects files that differ only in their last byte', () => {
    const content = Buffer.alloc(200_000, 'pnpm')
    const altered = Buffer.from(content)
    altered[altered.length - 1] = 0
    assert.equal(sameFileContents(write('long', content), write('altered', altered)), false)
  })

  test('rejects a file of a different length', () => {
    assert.equal(sameFileContents(write('short', 'pnpm'), write('shorter', 'pnp')), false)
  })

  test('rejects a destination that is missing, a directory, or a symlink', () => {
    const file = write('subject', 'pnpm')
    fs.mkdirSync(path.join(dir, 'as-directory'))
    fs.symlinkSync(file, path.join(dir, 'as-symlink'))

    assert.equal(sameFileContents(file, path.join(dir, 'absent')), false)
    assert.equal(sameFileContents(file, path.join(dir, 'as-directory')), false)
    assert.equal(sameFileContents(file, path.join(dir, 'as-symlink')), false)
  })
})

describe('renderUndoHint', () => {
  test('points at the shell config block on POSIX', () => {
    assert.equal(
      renderUndoHint('/home/me/.local/share/pnpm', 'linux'),
      'To uninstall, delete /home/me/.local/share/pnpm and the lines pnpm setup added to the shell config file named above.\n' +
      'See https://pnpm.io/uninstall'
    )
  })

  test('names only PNPM_HOME when setup left PATH alone', () => {
    for (const platform of ['linux', 'win32'] as const) {
      assert.equal(
        renderUndoHint('/home/me/.local/share/pnpm', platform, { modifiedPath: false }),
        'To uninstall, delete /home/me/.local/share/pnpm.\nSee https://pnpm.io/uninstall'
      )
    }
  })

  test('points at the user environment variables on Windows', () => {
    assert.equal(
      renderUndoHint('C:\\Users\\me\\AppData\\Local\\pnpm', 'win32'),
      'To uninstall, delete C:\\Users\\me\\AppData\\Local\\pnpm, then remove the PNPM_HOME variable and the Path entry pnpm setup added from your user environment variables.\n' +
      'See https://pnpm.io/uninstall'
    )
  })
})

describe('parseCliArgs', () => {
  test('adds pnpm to PATH unless --no-modify-path or PNPM_NO_MODIFY_PATH says otherwise', () => {
    assert.deepEqual(parseCliArgs(['12'], {}), { help: false, version: '12', modifyPath: true })
    assert.deepEqual(parseCliArgs(['--no-modify-path', '12'], {}), { help: false, version: '12', modifyPath: false })
    for (const value of ['1', 'true', 'yes']) {
      assert.equal(parseCliArgs([], { PNPM_NO_MODIFY_PATH: value }).modifyPath, false, value)
    }
    for (const value of ['', '0', 'false', 'FALSE']) {
      assert.equal(parseCliArgs([], { PNPM_NO_MODIFY_PATH: value }).modifyPath, true, value)
    }
  })

  test('refuses an unknown option and a second version', () => {
    assert.throws(() => parseCliArgs(['--modify-path'], {}), /Unknown option "--modify-path"/)
    assert.throws(() => parseCliArgs(['11', '12'], {}), /Expected at most one version, got 2/)
  })

  test('stops at --help', () => {
    assert.equal(parseCliArgs(['--help', 'extra', 'args'], {}).help, true)
  })
})

describe('setupCommand', () => {
  test('forces the built-in setup on the versions that let a project script replace it', () => {
    assert.deepEqual(setupCommand('10.33.0'), ['setup'])
    assert.deepEqual(setupCommand('11.0.0-beta.5'), ['setup'])
    assert.deepEqual(setupCommand('11.0.0-beta.6'), ['pm', 'setup'])
    assert.deepEqual(setupCommand('11.20.0'), ['pm', 'setup'])
    assert.deepEqual(setupCommand('12.0.0'), ['setup'])
    assert.deepEqual(setupCommand('12.1.0'), ['pm', 'setup'])
    assert.deepEqual(setupCommand('12.16.0'), ['pm', 'setup'])
  })
})
