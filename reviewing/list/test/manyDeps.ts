import { list } from '@pnpm/list'
import { fixtures } from '@pnpm/test-fixtures'

const f = fixtures(import.meta.dirname)
const fixtureWithManyDeps = f.find('many-deps')

test('list all deps in a project with many dependencies without failing with an OOM error', async () => {
  expect(await list([fixtureWithManyDeps], {
    checkWantedLockfileOnly: true,
    depth: Infinity,
    lockfileDir: fixtureWithManyDeps,
    virtualStoreDirMaxLength: 120,
    reportAs: 'json',
  })).toBeTruthy()
})
