import path from 'path'
import { createAllowBuildFunction } from '@pnpm/builder.policy'

it('should neverBuiltDependencies', () => {
  const allowBuild = createAllowBuildFunction({
    neverBuiltDependencies: ['foo'],
  })
  expect(typeof allowBuild).toBe('function')
  if (allowBuild) {
    expect(allowBuild('foo', '1.0.0')).toBeFalsy()
    expect(allowBuild('bar', '1.0.0')).toBeTruthy()
  }
})

it('should onlyBuiltDependencies', () => {
  const allowBuild = createAllowBuildFunction({
    onlyBuiltDependencies: ['foo'],
  })
  expect(typeof allowBuild).toBe('function')
  if (allowBuild) {
    expect(allowBuild('foo', '1.0.0')).toBeTruthy()
    expect(allowBuild('bar', '1.0.0')).toBeFalsy()
  }
})

it('should onlyBuiltDependencies set via a file', () => {
  const allowBuild = createAllowBuildFunction({
    onlyBuiltDependenciesFile: path.join(__dirname, 'onlyBuild.json'),
  })
  expect(typeof allowBuild).toBe('function')
  if (allowBuild) {
    expect(allowBuild('zoo', '1.0.0')).toBeTruthy()
    expect(allowBuild('qar', '1.0.0')).toBeTruthy()
    expect(allowBuild('bar', '1.0.0')).toBeFalsy()
  }
})

it('should onlyBuiltDependencies set via a file and config', () => {
  const allowBuild = createAllowBuildFunction({
    onlyBuiltDependencies: ['bar'],
    onlyBuiltDependenciesFile: path.join(__dirname, 'onlyBuild.json'),
  })
  expect(typeof allowBuild).toBe('function')
  if (allowBuild) {
    expect(allowBuild('zoo', '1.0.0')).toBeTruthy()
    expect(allowBuild('qar', '1.0.0')).toBeTruthy()
    expect(allowBuild('bar', '1.0.0')).toBeTruthy()
    expect(allowBuild('esbuild', '1.0.0')).toBeFalsy()
  }
})

it('should return undefined if no policy is set', () => {
  expect(createAllowBuildFunction({})).toBeUndefined()
})
