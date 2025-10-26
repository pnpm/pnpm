import { fixtures } from '@pnpm/test-fixtures'
import { readLocalConfig } from '@pnpm/config'

const f = fixtures(__dirname)

test('readLocalConfig parse number field', async () => {
  const config = await readLocalConfig(f.find('has-number-setting'))
  expect(typeof config.childConcurrency).toBe('number')
})
