import { expect, test } from '@jest/globals'
import { unresolvedOptionalDependencies } from '@pnpm/lockfile.verification'

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
