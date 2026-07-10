import { describe, expect, test } from '@jest/globals'

import { includeForEnvironment } from '../src/scan.js'

describe('includeForEnvironment — policy-only scope', () => {
  test('prod scopes to prod + optional, no dev', () => {
    expect(includeForEnvironment('prod')).toEqual({ dependencies: true, devDependencies: false, optionalDependencies: true })
  })
  test('dev scopes to dev only', () => {
    expect(includeForEnvironment('dev')).toEqual({ dependencies: false, devDependencies: true, optionalDependencies: false })
  })
  test('all includes everything regardless of any CLI flags', () => {
    expect(includeForEnvironment('all')).toEqual({ dependencies: true, devDependencies: true, optionalDependencies: true })
  })
})
