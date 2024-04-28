import fs from 'fs'
import { addDependenciesToPackage } from '@pnpm/core'
import { prepareEmpty } from '@pnpm/prepare'
import { testDefaults } from '../utils'

test('setting a custom virtual store directory max length', async () => {
  prepareEmpty()

  await addDependenciesToPackage({}, ['@babel/helper-member-expression-to-functions@7.23.0'], testDefaults({
    virtualStoreDirMaxLength: 50,
  }))

  const dirs = fs.readdirSync('node_modules/.pnpm')
  for (const dir of dirs) {
    expect(dir.length).toBeLessThanOrEqual(50)
  }
})
