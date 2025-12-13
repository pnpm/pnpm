import { fixtures } from '@pnpm/test-fixtures'
import { readLocalConfig } from '@pnpm/config'

const f = fixtures(import.meta.dirname)

test('readLocalConfig parse number field', async () => {
  const config = await readLocalConfig(f.find('local-config'))
  expect(config).toStrictEqual({
    modulesDir: 'node_modules',
    saveExact: false,
    savePrefix: '^',
  })
})
