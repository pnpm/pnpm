import { type SupportedArchitectures } from '@pnpm/types'
import { type CliOptions, type TargetConfig, overrideSupportedArchitecturesWithCLI } from '../src/overrideSupportedArchitecturesWithCLI'

function getOverriddenSupportedArchitectures (config: CliOptions & TargetConfig): SupportedArchitectures | undefined {
  overrideSupportedArchitecturesWithCLI(config)
  return config.supportedArchitectures
}

test('no flags, no overrides', () => {
  expect(getOverriddenSupportedArchitectures({})).toBeUndefined()

  expect(getOverriddenSupportedArchitectures({
    supportedArchitectures: {},
  })).toStrictEqual({})

  expect(getOverriddenSupportedArchitectures({
    supportedArchitectures: {
      os: ['linux'],
    }
  })).toStrictEqual({
    os: ['linux'],
  } as SupportedArchitectures)

  expect(getOverriddenSupportedArchitectures({
    supportedArchitectures: {
      cpu: ['x64'],
      os: ['linux'],
    }
  })).toStrictEqual({
    cpu: ['x64'],
    os: ['linux'],
  } as SupportedArchitectures)

  expect(getOverriddenSupportedArchitectures({
    supportedArchitectures: {
      cpu: ['x64'],
      libc: ['glibc'],
      os: ['linux'],
    }
  })).toStrictEqual({
    cpu: ['x64'],
    libc: ['glibc'],
    os: ['linux'],
  } as SupportedArchitectures)
})

test('overrides', () => {
  expect(getOverriddenSupportedArchitectures({
    cpu: ['arm64'],
    os: ['darwin'],
  })).toStrictEqual({
    cpu: ['arm64'],
    os: ['darwin'],
  })

  expect(getOverriddenSupportedArchitectures({
    cpu: ['arm64'],
    os: ['darwin'],
    supportedArchitectures: {},
  })).toStrictEqual({
    cpu: ['arm64'],
    os: ['darwin'],
  })

  expect(getOverriddenSupportedArchitectures({
    cpu: ['arm64'],
    os: ['darwin'],
    supportedArchitectures: {
      os: ['linux'],
    }
  })).toStrictEqual({
    cpu: ['arm64'],
    os: ['darwin'],
  } as SupportedArchitectures)

  expect(getOverriddenSupportedArchitectures({
    cpu: ['arm64'],
    os: ['darwin'],
    supportedArchitectures: {
      cpu: ['x64'],
      os: ['linux'],
    }
  })).toStrictEqual({
    cpu: ['arm64'],
    os: ['darwin'],
  } as SupportedArchitectures)

  expect(getOverriddenSupportedArchitectures({
    cpu: ['arm64'],
    os: ['darwin'],
    supportedArchitectures: {
      cpu: ['x64'],
      libc: ['glibc'],
      os: ['linux'],
    }
  })).toStrictEqual({
    cpu: ['arm64'],
    libc: ['glibc'],
    os: ['darwin'],
  } as SupportedArchitectures)
})
