import { type SupportedArchitectures } from '@pnpm/types'
import { type CliOptions, type TargetConfig, overrideSupportedArchitecturesWithCLI } from '../src/overrideSupportedArchitecturesWithCLI.js'

function getOverriddenSupportedArchitectures (
  supportedArchitectures: SupportedArchitectures | undefined,
  cliOptions: CliOptions
): SupportedArchitectures | undefined {
  const config: TargetConfig = { supportedArchitectures }
  overrideSupportedArchitecturesWithCLI(config, cliOptions)
  return config.supportedArchitectures
}

test('no flags, no overrides', () => {
  expect(getOverriddenSupportedArchitectures(undefined, {})).toBeUndefined()

  expect(getOverriddenSupportedArchitectures({}, {})).toStrictEqual({})

  expect(getOverriddenSupportedArchitectures({
    os: ['linux'],
  }, {})).toStrictEqual({
    os: ['linux'],
  } as SupportedArchitectures)

  expect(getOverriddenSupportedArchitectures({
    cpu: ['x64'],
    os: ['linux'],
  }, {})).toStrictEqual({
    cpu: ['x64'],
    os: ['linux'],
  } as SupportedArchitectures)

  expect(getOverriddenSupportedArchitectures({
    cpu: ['x64'],
    libc: ['glibc'],
    os: ['linux'],
  }, {})).toStrictEqual({
    cpu: ['x64'],
    libc: ['glibc'],
    os: ['linux'],
  } as SupportedArchitectures)
})

test('overrides', () => {
  expect(getOverriddenSupportedArchitectures(undefined, {
    cpu: ['arm64'],
    os: ['darwin'],
  })).toStrictEqual({
    cpu: ['arm64'],
    os: ['darwin'],
  })

  expect(getOverriddenSupportedArchitectures({}, {
    cpu: ['arm64'],
    os: ['darwin'],
  })).toStrictEqual({
    cpu: ['arm64'],
    os: ['darwin'],
  })

  expect(getOverriddenSupportedArchitectures({
    os: ['linux'],
  }, {
    cpu: ['arm64'],
    os: ['darwin'],
  })).toStrictEqual({
    cpu: ['arm64'],
    os: ['darwin'],
  } as SupportedArchitectures)

  expect(getOverriddenSupportedArchitectures({
    cpu: ['x64'],
    os: ['linux'],
  }, {
    cpu: ['arm64'],
    os: ['darwin'],
  })).toStrictEqual({
    cpu: ['arm64'],
    os: ['darwin'],
  } as SupportedArchitectures)

  expect(getOverriddenSupportedArchitectures({
    cpu: ['x64'],
    libc: ['glibc'],
    os: ['linux'],
  }, {
    cpu: ['arm64'],
    os: ['darwin'],
  })).toStrictEqual({
    cpu: ['arm64'],
    libc: ['glibc'],
    os: ['darwin'],
  } as SupportedArchitectures)
})
