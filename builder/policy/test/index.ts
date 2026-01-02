import { createAllowBuildFunction } from '@pnpm/builder.policy'

it('should onlyBuiltDependencies', () => {
  const allowBuild = createAllowBuildFunction({
    onlyBuiltDependencies: ['foo', 'qar@1.0.0 || 2.0.0'],
  })
  expect(typeof allowBuild).toBe('function')
  expect(allowBuild!('foo', '1.0.0')).toBeTruthy()
  expect(allowBuild!('bar', '1.0.0')).toBeFalsy()
  expect(allowBuild!('qar', '1.1.0')).toBeFalsy()
  expect(allowBuild!('qar', '1.0.0')).toBeTruthy()
  expect(allowBuild!('qar', '2.0.0')).toBeTruthy()
})

it('should not allow patterns in onlyBuiltDependencies', () => {
  const allowBuild = createAllowBuildFunction({
    onlyBuiltDependencies: ['is-*'],
  })
  expect(typeof allowBuild).toBe('function')
  expect(allowBuild!('is-odd', '1.0.0')).toBeFalsy()
})

it('should return undefined if no policy is set', () => {
  expect(createAllowBuildFunction({})).toBeUndefined()
})

it('should allow everything when dangerouslyAllowAllBuilds is true', () => {
  const allowBuild = createAllowBuildFunction({
    dangerouslyAllowAllBuilds: true,
  })
  expect(typeof allowBuild).toBe('function')
  expect(allowBuild!('foo', '1.0.0')).toBeTruthy()
})
