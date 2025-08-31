import { workspacePrefToNpm } from '../lib/workspacePrefToNpm.js'

describe('workspacePrefToNpm', () => {
  test('resolve workspace only version aliases', async () => {
    expect(workspacePrefToNpm('workspace:^')).toBe('*')
    expect(workspacePrefToNpm('workspace:~')).toBe('*')
  })

  test('resolve package name aliases', async () => {
    expect(workspacePrefToNpm('workspace:is-positive@3.0.0')).toBe('npm:is-positive@3.0.0')
    expect(workspacePrefToNpm('workspace:is-positive@*')).toBe('npm:is-positive@*')
    expect(workspacePrefToNpm('workspace:is-positive@^')).toBe('npm:is-positive@*')
  })

  test('resolve scoped package name aliases', async () => {
    expect(
      workspacePrefToNpm('workspace:@scope/is-positive@1.2.3')
    ).toBe('npm:@scope/is-positive@1.2.3')
    expect(
      workspacePrefToNpm('workspace:@scope/is-positive@^1.2.3')
    ).toBe('npm:@scope/is-positive@^1.2.3')
    expect(
      workspacePrefToNpm('workspace:@scope/is-positive@*')
    ).toBe('npm:@scope/is-positive@*')
    expect(
      workspacePrefToNpm('workspace:@scope/is-positive@~')
    ).toBe('npm:@scope/is-positive@*')
  })
})
