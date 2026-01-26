import { resolveWorkspaceRange } from '@pnpm/resolve-workspace-range'

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
})
