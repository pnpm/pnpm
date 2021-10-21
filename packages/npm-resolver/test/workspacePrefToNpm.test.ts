import workspacePrefToNpm from '@pnpm/npm-resolver/lib/workspacePrefToNpm'

describe('workspacePrefToNpm', () => {
  test('resolve workspace only version aliases', async () => {
    expect(workspacePrefToNpm('workspace:^')).toStrictEqual('*')
    expect(workspacePrefToNpm('workspace:~')).toStrictEqual('*')
  })

  test('resolve package name aliases', async () => {
    expect(workspacePrefToNpm('workspace:is-positive@3.0.0')).toStrictEqual('npm:is-positive@3.0.0')
    expect(workspacePrefToNpm('workspace:is-positive@*')).toStrictEqual('npm:is-positive@*')
    expect(workspacePrefToNpm('workspace:is-positive@^')).toStrictEqual('npm:is-positive@*')
  })

  test('resolve scoped package name aliases', async () => {
    expect(
      workspacePrefToNpm('workspace:@scope/is-positive@1.2.3')
    ).toStrictEqual('npm:@scope/is-positive@1.2.3')
    expect(
      workspacePrefToNpm('workspace:@scope/is-positive@^1.2.3')
    ).toStrictEqual('npm:@scope/is-positive@^1.2.3')
    expect(
      workspacePrefToNpm('workspace:@scope/is-positive@*')
    ).toStrictEqual('npm:@scope/is-positive@*')
    expect(
      workspacePrefToNpm('workspace:@scope/is-positive@~')
    ).toStrictEqual('npm:@scope/is-positive@*')
  })
})
