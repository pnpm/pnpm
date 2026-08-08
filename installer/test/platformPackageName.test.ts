import { expect, test } from '@jest/globals'
import { platformPackageName, type Target } from 'get-pnpm'

function target (overrides: Partial<Target>): Target {
  return { major: 11, platform: 'linux', arch: 'x64', libcFamily: 'glibc', ...overrides }
}

test('v11 and older use the legacy platform package names', () => {
  expect(platformPackageName(target({}))).toBe('@pnpm/linux-x64')
  expect(platformPackageName(target({ libcFamily: 'musl' }))).toBe('@pnpm/linuxstatic-x64')
  expect(platformPackageName(target({ platform: 'darwin', arch: 'arm64' }))).toBe('@pnpm/macos-arm64')
  expect(platformPackageName(target({ platform: 'win32' }))).toBe('@pnpm/win-x64')
  expect(platformPackageName(target({ major: 10 }))).toBe('@pnpm/linux-x64')
})

test('v12 and newer use the process.platform-based names', () => {
  expect(platformPackageName(target({ major: 12 }))).toBe('@pnpm/exe.linux-x64')
  expect(platformPackageName(target({ major: 12, libcFamily: 'musl' }))).toBe('@pnpm/exe.linux-x64-musl')
  expect(platformPackageName(target({ major: 12, platform: 'darwin', arch: 'x64' }))).toBe('@pnpm/exe.darwin-x64')
  expect(platformPackageName(target({ major: 12, platform: 'win32', arch: 'arm64' }))).toBe('@pnpm/exe.win32-arm64')
})

test('the musl suffix is Linux-only', () => {
  expect(platformPackageName(target({ platform: 'darwin', arch: 'arm64', libcFamily: 'musl' }))).toBe('@pnpm/macos-arm64')
  expect(platformPackageName(target({ major: 12, platform: 'win32', libcFamily: 'musl' }))).toBe('@pnpm/exe.win32-x64')
})

test('rejects hosts pnpm publishes no binary for', () => {
  expect(() => platformPackageName(target({ arch: 'ia32' }))).toThrow(/x86_64\/arm64/)
  expect(() => platformPackageName(target({ platform: 'freebsd' }))).toThrow(/does not provide a pre-built binary for freebsd/)
})

test('points Intel macOS users away from v11, which has no working binary', () => {
  expect(() => platformPackageName(target({ platform: 'darwin', arch: 'x64' })))
    .toThrow(/Intel macOS.*npx get-pnpm 12/s)
  expect(platformPackageName(target({ major: 10, platform: 'darwin', arch: 'x64' }))).toBe('@pnpm/macos-x64')
})
