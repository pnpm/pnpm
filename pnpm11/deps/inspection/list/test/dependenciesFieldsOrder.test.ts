import { expect, test } from '@jest/globals'
import { DEPENDENCIES_FIELDS } from '@pnpm/types'

import { renderJson } from '../lib/renderJson.js'
import { renderTree } from '../lib/renderTree.js'

const project = {
  name: 'project',
  version: '1.0.0',
  path: '/project',
  private: false,
  dependencies: [],
}

test('rendering does not reorder the shared DEPENDENCIES_FIELDS constant', async () => {
  const originalOrder = [...DEPENDENCIES_FIELDS]

  await renderTree([project], {
    alwaysPrintRootPackage: true,
    depth: 0,
    long: false,
    search: false,
    showExtraneous: false,
  })
  await renderJson([project], { depth: 0, long: false, search: false })

  expect(DEPENDENCIES_FIELDS).toStrictEqual(originalOrder)
})
