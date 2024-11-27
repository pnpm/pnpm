import { WorkspaceSpec } from '../src/index'

test('parse valid workspace spec', () => {
  expect(WorkspaceSpec.parse('workspace:*')).toStrictEqual(new WorkspaceSpec('*'))
  expect(WorkspaceSpec.parse('workspace:^')).toStrictEqual(new WorkspaceSpec('^'))
  expect(WorkspaceSpec.parse('workspace:~')).toStrictEqual(new WorkspaceSpec('~'))
  expect(WorkspaceSpec.parse('workspace:0.1.2')).toStrictEqual(new WorkspaceSpec('0.1.2'))
  expect(WorkspaceSpec.parse('workspace:foo@*')).toStrictEqual(new WorkspaceSpec('*', 'foo'))
  expect(WorkspaceSpec.parse('workspace:foo@^')).toStrictEqual(new WorkspaceSpec('^', 'foo'))
  expect(WorkspaceSpec.parse('workspace:foo@~')).toStrictEqual(new WorkspaceSpec('~', 'foo'))
  expect(WorkspaceSpec.parse('workspace:foo@0.1.2')).toStrictEqual(new WorkspaceSpec('0.1.2', 'foo'))
  expect(WorkspaceSpec.parse('workspace:@foo/bar@*')).toStrictEqual(new WorkspaceSpec('*', '@foo/bar'))
  expect(WorkspaceSpec.parse('workspace:@foo/bar@^')).toStrictEqual(new WorkspaceSpec('^', '@foo/bar'))
  expect(WorkspaceSpec.parse('workspace:@foo/bar@~')).toStrictEqual(new WorkspaceSpec('~', '@foo/bar'))
  expect(WorkspaceSpec.parse('workspace:@foo/bar@0.1.2')).toStrictEqual(new WorkspaceSpec('0.1.2', '@foo/bar'))
})

test('parse invalid workspace spec', () => {
  expect(WorkspaceSpec.parse('npm:foo@0.1.2')).toBe(null)
  expect(WorkspaceSpec.parse('*')).toBe(null)
})

test('to string', () => {
  expect(new WorkspaceSpec('*').toString()).toBe('workspace:*')
  expect(new WorkspaceSpec('^').toString()).toBe('workspace:^')
  expect(new WorkspaceSpec('~').toString()).toBe('workspace:~')
  expect(new WorkspaceSpec('0.1.2').toString()).toBe('workspace:0.1.2')
  expect(new WorkspaceSpec('*', 'foo').toString()).toBe('workspace:foo@*')
  expect(new WorkspaceSpec('^', 'foo').toString()).toBe('workspace:foo@^')
  expect(new WorkspaceSpec('~', 'foo').toString()).toBe('workspace:foo@~')
  expect(new WorkspaceSpec('0.1.2', 'foo').toString()).toBe('workspace:foo@0.1.2')
  expect(new WorkspaceSpec('*', '@foo/bar').toString()).toBe('workspace:@foo/bar@*')
  expect(new WorkspaceSpec('^', '@foo/bar').toString()).toBe('workspace:@foo/bar@^')
  expect(new WorkspaceSpec('~', '@foo/bar').toString()).toBe('workspace:@foo/bar@~')
  expect(new WorkspaceSpec('0.1.2', '@foo/bar').toString()).toBe('workspace:@foo/bar@0.1.2')
})

test('mutate alias and version', () => {
  const spec = WorkspaceSpec.parse('workspace:*')!
  expect(spec.toString()).toBe('workspace:*')
  spec.version = '^'
  expect(spec.toString()).toBe('workspace:^')
  spec.alias = 'foo'
  expect(spec.toString()).toBe('workspace:foo@^')
  delete spec.alias
  expect(spec.toString()).toBe('workspace:^')
})
