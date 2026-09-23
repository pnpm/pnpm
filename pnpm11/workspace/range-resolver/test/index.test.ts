import { describe, expect, test } from '@jest/globals'
import { resolveWorkspaceRange } from '@pnpm/workspace.range-resolver'

describe('resolveWorkspaceRange', () => {
  const versions = ['1.0.0', '2.0.0', '3.0.0-beta.1']

  test('resolves * to max version including prereleases', () => {
    expect(resolveWorkspaceRange('*', versions)).toBe('3.0.0-beta.1')
  })

  test('resolves ^ to max version including prereleases', () => {
    expect(resolveWorkspaceRange('^', versions)).toBe('3.0.0-beta.1')
  })

  test('resolves ~ to max version including prereleases', () => {
    expect(resolveWorkspaceRange('~', versions)).toBe('3.0.0-beta.1')
  })

  test('resolves empty string (bare workspace:) to max version including prereleases', () => {
    expect(resolveWorkspaceRange('', versions)).toBe('3.0.0-beta.1')
  })

  test('resolves semver range', () => {
    expect(resolveWorkspaceRange('^1.0.0', versions)).toBe('1.0.0')
    expect(resolveWorkspaceRange('^2.0.0', versions)).toBe('2.0.0')
    expect(resolveWorkspaceRange('>=1.0.0', versions)).toBe('2.0.0')
  })

  test('returns null when no version satisfies range', () => {
    expect(resolveWorkspaceRange('^4.0.0', versions)).toBeNull()
  })

  test('resolves wildcards to a non-semver version when no semver version is present', () => {
    expect(resolveWorkspaceRange('*', ['1'])).toBe('1')
    expect(resolveWorkspaceRange('^', ['1.0'])).toBe('1.0')
    expect(resolveWorkspaceRange('*', ['1', '2'])).toBe('2')
    expect(resolveWorkspaceRange('*', ['1', '1.0.0'])).toBe('1.0.0')
  })

  test('resolves a range identical to a non-semver version', () => {
    expect(resolveWorkspaceRange('1', ['1'])).toBe('1')
    expect(resolveWorkspaceRange('1.0', ['1.0', '2'])).toBe('1.0')
    expect(resolveWorkspaceRange('2', ['1'])).toBeNull()
  })
})
