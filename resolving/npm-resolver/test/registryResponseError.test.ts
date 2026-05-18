import { describe, expect, test } from '@jest/globals'

import { RegistryResponseError } from '../src/fetch.js'

const request = { url: 'https://registry.npmjs.org/foo' }
const notFoundResponse = { status: 404, statusText: 'Not Found' }

describe('RegistryResponseError hint', () => {
  test('suggests stripped name when pkg has "@<version>" suffix', () => {
    const err = new RegistryResponseError(request, notFoundResponse, 'lodash@4.17.21')
    expect(err.hint).toContain('Did you mean lodash?')
  })

  test('suggests stripped name when pkg has trailing "X.Y.Z" without "@"', () => {
    const err = new RegistryResponseError(request, notFoundResponse, 'lodash4.17.21')
    expect(err.hint).toContain('Did you mean lodash?')
  })

  test('handles scoped names with version suffix', () => {
    const err = new RegistryResponseError(request, notFoundResponse, '@scope/foo@1.2.3')
    expect(err.hint).toContain('Did you mean @scope/foo?')
  })

  test('does not add a suggestion for bare scoped names', () => {
    const err = new RegistryResponseError(request, notFoundResponse, '@scope/foo')
    expect(err.hint).not.toMatch(/Did you mean/)
  })

  test('does not add a suggestion when the prefix would be empty', () => {
    const err = new RegistryResponseError(request, notFoundResponse, '1.0.0')
    expect(err.hint).not.toMatch(/Did you mean/)
  })

  test('does not add a suggestion for a plain unversioned name', () => {
    const err = new RegistryResponseError(request, notFoundResponse, 'foo')
    expect(err.hint).not.toMatch(/Did you mean/)
  })

  test('does not emit a hint on non-404 responses', () => {
    const err = new RegistryResponseError(
      request,
      { status: 500, statusText: 'Internal Server Error' },
      'foo@1.0.0'
    )
    expect(err.hint ?? '').not.toMatch(/Did you mean/)
  })

  test('stays linear under adversarial input', () => {
    // The previous implementation used a regex with super-linear backtracking.
    // 10k repetitions of "1" should still complete promptly.
    const pkgName = '1'.repeat(10_000)
    const start = Date.now()
    const err = new RegistryResponseError(request, notFoundResponse, pkgName)
    const elapsed = Date.now() - start
    expect(elapsed).toBeLessThan(1_000)
    // The all-digits input has no usable name prefix, so no suggestion is added.
    expect(err.hint).not.toMatch(/Did you mean/)
  })
})
