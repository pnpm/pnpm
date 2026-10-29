import { expect, test } from '@jest/globals'
import { unresolvedOptionalDependencies } from '@pnpm/lockfile.verification'
import type { WorkspacePackages } from '@pnpm/resolving.resolver-base'
import type { ProjectRootDir } from '@pnpm/types'

test('unresolvedOptionalDependencies() returns the optional dependencies the importer has no entry for', () => {
  const importer = {
    specifiers: {
      recorded: '1.0.0',
    },
  }
  const pkg = {
    optionalDependencies: {
      '@ignored/pkg': '1.0.0',
      linked: 'link:../linked',
      recorded: '1.0.0',
      unresolvable: '^30000.0.0',
    },
  }
  expect(unresolvedOptionalDependencies({}, importer, pkg)).toStrictEqual({
    '@ignored/pkg': '1.0.0',
    linked: 'link:../linked',
    unresolvable: '^30000.0.0',
  })
  expect(unresolvedOptionalDependencies({
    excludeLinksFromLockfile: true,
    ignoredOptionalDependencies: ['@ignored/*'],
  }, importer, pkg)).toStrictEqual({
    unresolvable: '^30000.0.0',
  })
  expect(unresolvedOptionalDependencies({}, importer, {})).toStrictEqual({})
})

test('unresolvedOptionalDependencies() leaves out a workspace link that excludeLinksFromLockfile omitted', () => {
  const workspacePackages: WorkspacePackages = new Map([
    ['bar', new Map([
      ['1.2.0', { rootDir: '/workspace/bar' as ProjectRootDir, manifest: { name: 'bar', version: '1.2.0' } }],
    ])],
  ])
  const pkg = {
    optionalDependencies: {
      bar: '^1.0.0',
      unresolvable: '^30000.0.0',
    },
  }
  expect(unresolvedOptionalDependencies({ excludeLinksFromLockfile: true, workspacePackages }, { specifiers: {} }, pkg)).toStrictEqual({
    unresolvable: '^30000.0.0',
  })
  expect(unresolvedOptionalDependencies({ workspacePackages }, { specifiers: {} }, pkg)).toStrictEqual(pkg.optionalDependencies)
})
