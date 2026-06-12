import { expect, jest, test } from '@jest/globals'
import type * as DetectLibc from 'detect-libc'

jest.mock('detect-libc', () => {
  const original = jest.requireActual<typeof DetectLibc>('detect-libc')
  return {
    ...original,
    familySync: () => 'glibc',
  }
})

const { inferPlatformFromPackageName } = await import('../lib/inferPlatformFromPackageName.js')
const { packageIsInstallable } = await import('../lib/index.js')

test.each([
  ['@nx/nx-win32-arm64-msvc', { os: ['win32'], cpu: ['arm64'] }],
  ['@nx/nx-linux-arm-gnueabihf', { os: ['linux'], cpu: ['arm'], libc: ['glibc'] }],
  ['@nx/nx-linux-x64-gnu', { os: ['linux'], cpu: ['x64'], libc: ['glibc'] }],
  ['@esbuild/aix-ppc64', { os: ['aix'], cpu: ['ppc64'] }],
  ['@esbuild/openharmony-arm64', { os: ['openharmony'], cpu: ['arm64'] }],
  ['@biomejs/cli-linux-x64-musl', { os: ['linux'], cpu: ['x64'], libc: ['musl'] }],
  ['@typescript/native-preview-darwin-arm64', { os: ['darwin'], cpu: ['arm64'] }],
  ['turbo-windows-64', { os: ['win32'] }],
  ['esbuild-darwin-64', { os: ['darwin'] }],
  ['bun-linux-aarch64', { os: ['linux'], cpu: ['arm64'] }],
  ['sharp-linux-armv7', { os: ['linux'], cpu: ['arm'] }],
  ['is-arm', { cpu: ['arm'] }],
  ['fsevents', null],
  ['lodash', null],
  ['@pnpm.e2e/not-compatible-with-any-os', null],
])('inferPlatformFromPackageName(%s)', (name, inferred) => {
  expect(inferPlatformFromPackageName(name)).toStrictEqual(inferred)
})

test('an optional dependency without platform fields is not installable when its name declares an unsupported platform', () => {
  expect(packageIsInstallable('@nx/nx-win32-arm64-msvc@1.0.0', {
    name: '@nx/nx-win32-arm64-msvc',
    version: '1.0.0',
  }, {
    optional: true,
    lockfileDir: process.cwd(),
    supportedArchitectures: { os: ['linux'], cpu: ['x64'] },
  })).toBe(false)
})

test('a missing libc field is taken from the package name even when the other platform fields are declared', () => {
  const options = {
    optional: true,
    lockfileDir: process.cwd(),
    supportedArchitectures: { os: ['linux'], cpu: ['x64'], libc: ['glibc'] },
  }
  expect(packageIsInstallable('@nx/nx-linux-x64-musl@1.0.0', {
    name: '@nx/nx-linux-x64-musl',
    version: '1.0.0',
    os: ['linux'],
    cpu: ['x64'],
  }, options)).toBe(false)
  expect(packageIsInstallable('@nx/nx-linux-x64-gnu@1.0.0', {
    name: '@nx/nx-linux-x64-gnu',
    version: '1.0.0',
    os: ['linux'],
    cpu: ['x64'],
  }, options)).toBe(true)
})

test('a missing cpu field is taken from the name of a package that declares its platform', () => {
  expect(packageIsInstallable('@pnpm.e2e/some-pkg-arm64@1.0.0', {
    name: '@pnpm.e2e/some-pkg-arm64',
    version: '1.0.0',
    os: ['linux'],
  }, {
    optional: true,
    lockfileDir: process.cwd(),
    supportedArchitectures: { os: ['linux'], cpu: ['x64'] },
  })).toBe(false)
})

test('the platform fields of the manifest take precedence over the package name', () => {
  expect(packageIsInstallable('@pnpm.e2e/win32-binary@1.0.0', {
    name: '@pnpm.e2e/win32-binary',
    version: '1.0.0',
    os: ['linux'],
    cpu: ['x64'],
    libc: ['glibc'],
  }, {
    optional: true,
    lockfileDir: process.cwd(),
    supportedArchitectures: { os: ['linux'], cpu: ['x64'], libc: ['glibc'] },
  })).toBe(true)
})

test('a package without any declared platform field is not skipped when its name has no operating system token', () => {
  expect(packageIsInstallable('is-arm@1.0.0', {
    name: 'is-arm',
    version: '1.0.0',
  }, {
    optional: true,
    lockfileDir: process.cwd(),
    supportedArchitectures: { os: ['linux'], cpu: ['x64'] },
  })).toBe(true)
})

test('the platform is not inferred from the name of a non-optional dependency', () => {
  expect(packageIsInstallable('@nx/nx-win32-arm64-msvc@1.0.0', {
    name: '@nx/nx-win32-arm64-msvc',
    version: '1.0.0',
  }, {
    optional: false,
    lockfileDir: process.cwd(),
    supportedArchitectures: { os: ['linux'], cpu: ['x64'] },
  })).toBe(true)
})
