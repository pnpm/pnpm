import { createAllowBuildFunction } from '@pnpm/builder.policy'

it('should allowBuilds with true value', () => {
  const allowBuild = createAllowBuildFunction({
    allowBuilds: { foo: true, 'qar@1.0.0 || 2.0.0': true },
  })
  expect(typeof allowBuild).toBe('function')
  expect(allowBuild!('foo', '1.0.0')).toBe(true)
  expect(allowBuild!('bar', '1.0.0')).toBeUndefined()
  expect(allowBuild!('qar', '1.1.0')).toBeUndefined()
  expect(allowBuild!('qar', '1.0.0')).toBe(true)
  expect(allowBuild!('qar', '2.0.0')).toBe(true)
})

it('should allowBuilds with false value', () => {
  const allowBuild = createAllowBuildFunction({
    allowBuilds: { foo: false, bar: true },
  })
  expect(typeof allowBuild).toBe('function')
  expect(allowBuild!('foo', '1.0.0')).toBe(false)
  expect(allowBuild!('bar', '1.0.0')).toBe(true)
  expect(allowBuild!('baz', '1.0.0')).toBeUndefined()
})

it('should not allow patterns in allowBuilds', () => {
  const allowBuild = createAllowBuildFunction({
    allowBuilds: { 'is-*': true },
  })
  expect(typeof allowBuild).toBe('function')
  expect(allowBuild!('is-odd', '1.0.0')).toBeUndefined()
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
