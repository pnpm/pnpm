import { addDependenciesToPackage } from '@pnpm/core'
import { prepareEmpty } from '@pnpm/prepare'
import { testDefaults } from '../utils'

test('auto install peer dependencies', async () => {
  prepareEmpty()
  await addDependenciesToPackage({}, ['abc'], await testDefaults())
})
