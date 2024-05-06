import { parseWantedDependency } from '@pnpm/parse-wanted-dependency'

test('parseWantedDependency()', () => {
  const wantedDep = parseWantedDependency('foo@file:../foo')
  expect(wantedDep.alias).toBe('foo')
  expect(wantedDep.pref).toBe('file:../foo')
})
