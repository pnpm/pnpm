import { beforeEach, expect, jest, test } from '@jest/globals'
import type { PackageResponse, RequestPackageFunction } from '@pnpm/store.controller-types'

jest.unstable_mockModule('@pnpm/logger', () => ({
  globalWarn: jest.fn(),
}))

const { globalWarn } = await import('@pnpm/logger')
const { warnOnStaleConvergenceOverrides } = await import('../../lib/install/warnOnStaleConvergenceOverrides.js')

beforeEach(() => {
  jest.mocked(globalWarn).mockClear()
})

function fakeRequestPackage (versionByRange: Record<string, string | Error>): RequestPackageFunction {
  return (async (wantedDependency: { alias?: string, bareSpecifier?: string }) => {
    const version = versionByRange[wantedDependency.bareSpecifier!]
    if (version == null) throw new Error(`Unexpected range ${wantedDependency.bareSpecifier!}`)
    if (version instanceof Error) throw version
    return {
      body: {
        manifest: { name: wantedDependency.alias, version },
      },
    } as unknown as PackageResponse
  }) as RequestPackageFunction
}

const FOO_CONVERGENCE_OVERRIDE = {
  selector: 'foo@',
  targetPkg: { name: 'foo', bareSpecifier: '' },
  newBareSpecifier: '4.0.6',
  converge: true,
}

test('warns when a newer version satisfies every declared range', async () => {
  await warnOnStaleConvergenceOverrides({
    convergeDeclaredRanges: new Map([['foo', new Set(['^4.0.5', '>=4.0.0 <5.0.0'])]]),
    parsedOverrides: [FOO_CONVERGENCE_OVERRIDE],
    requestPackage: fakeRequestPackage({
      '^4.0.5': '4.0.9',
      '>=4.0.0 <5.0.0': '4.0.9',
    }),
    lockfileDir: process.cwd(),
  })
  expect(globalWarn).toHaveBeenCalledTimes(1)
  const warning = jest.mocked(globalWarn).mock.calls[0][0]
  expect(warning).toContain('"foo@": "4.0.6" is stale')
  expect(warning).toContain('4.0.9')
})

test('does not warn when an exact declared range caps the convergence at the current value', async () => {
  await warnOnStaleConvergenceOverrides({
    convergeDeclaredRanges: new Map([['foo', new Set(['^4.0.5', '4.0.6'])]]),
    parsedOverrides: [FOO_CONVERGENCE_OVERRIDE],
    requestPackage: fakeRequestPackage({
      '^4.0.5': '4.0.9',
      '4.0.6': '4.0.6',
    }),
    lockfileDir: process.cwd(),
  })
  expect(globalWarn).not.toHaveBeenCalled()
})

test('does not warn when the value is already the best convergence', async () => {
  await warnOnStaleConvergenceOverrides({
    convergeDeclaredRanges: new Map([['foo', new Set(['^4.0.5'])]]),
    parsedOverrides: [FOO_CONVERGENCE_OVERRIDE],
    requestPackage: fakeRequestPackage({
      '^4.0.5': '4.0.6',
    }),
    lockfileDir: process.cwd(),
  })
  expect(globalWarn).not.toHaveBeenCalled()
})

test('still warns when one range fails to resolve but another range supplies an all-satisfying candidate', async () => {
  await warnOnStaleConvergenceOverrides({
    convergeDeclaredRanges: new Map([['foo', new Set(['^4.0.5', '^4.0.7'])]]),
    parsedOverrides: [FOO_CONVERGENCE_OVERRIDE],
    requestPackage: fakeRequestPackage({
      '^4.0.5': '4.0.9',
      '^4.0.7': new Error('registry down'),
    }),
    lockfileDir: process.cwd(),
  })
  expect(globalWarn).toHaveBeenCalledTimes(1)
  expect(jest.mocked(globalWarn).mock.calls[0][0]).toContain('4.0.9')
})

test('does not warn for packages without collected ranges', async () => {
  await warnOnStaleConvergenceOverrides({
    convergeDeclaredRanges: new Map(),
    parsedOverrides: [FOO_CONVERGENCE_OVERRIDE],
    requestPackage: fakeRequestPackage({}),
    lockfileDir: process.cwd(),
  })
  expect(globalWarn).not.toHaveBeenCalled()
})
