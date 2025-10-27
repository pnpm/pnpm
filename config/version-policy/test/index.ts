import { createPackageVersionPolicy } from '@pnpm/config.version-policy'

test('createPackageVersionPolicy()', () => {
  {
    const match = createPackageVersionPolicy(['axios@1.12.2'])
    expect(match('axios')).toStrictEqual(['1.12.2'])
  }
  {
    const match = createPackageVersionPolicy(['is-*'])
    expect(match('is-odd')).toBe(true)
    expect(match('is-even')).toBe(true)
    expect(match('lodash')).toBe(false)
  }
  {
    const match = createPackageVersionPolicy(['@babel/core@7.20.0'])
    expect(match('@babel/core')).toStrictEqual(['7.20.0'])
  }
  {
    const match = createPackageVersionPolicy(['@babel/core'])
    expect(match('@babel/core')).toBe(true)
  }
  {
    const match = createPackageVersionPolicy(['axios@1.12.2'])
    expect(match('is-odd')).toBe(false)
  }
  {
    const match = createPackageVersionPolicy(['axios@1.12.2', 'lodash@4.17.21', 'is-*'])
    expect(match('axios')).toStrictEqual(['1.12.2'])
    expect(match('lodash')).toStrictEqual(['4.17.21'])
    expect(match('is-odd')).toBe(true)
  }
  {
    expect(() => createPackageVersionPolicy(['lodash@^4.17.0'])).toThrow(/Invalid versions union/)
    expect(() => createPackageVersionPolicy(['lodash@~4.17.0'])).toThrow(/Invalid versions union/)
    expect(() => createPackageVersionPolicy(['react@>=18.0.0'])).toThrow(/Invalid versions union/)
    expect(() => createPackageVersionPolicy(['is-*@1.0.0'])).toThrow(/Name patterns are not allowed/)
  }
  {
    const match = createPackageVersionPolicy(['axios@1.12.0 || 1.12.1'])
    expect(match('axios')).toStrictEqual(['1.12.0', '1.12.1'])
  }
  {
    const match = createPackageVersionPolicy(['@scope/pkg@1.0.0 || 1.0.1'])
    expect(match('@scope/pkg')).toStrictEqual(['1.0.0', '1.0.1'])
  }
  {
    const match = createPackageVersionPolicy(['pkg@1.0.0||1.0.1  ||  1.0.2'])
    expect(match('pkg')).toStrictEqual(['1.0.0', '1.0.1', '1.0.2'])
  }
})
