import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { temporaryDirectory } from 'tempy'

import { getProjectNodePath } from '../src/getProjectNodePath.js'

test('a project reached through a symlinked ancestor gets no entry for its node_modules, and its custom modules directory keeps its spelling', async () => {
  const tmp = temporaryDirectory()
  fs.mkdirSync(path.join(tmp, 'real', 'node_modules'), { recursive: true })
  fs.mkdirSync(path.join(tmp, 'real', 'vendor'))
  const rootDir = path.join(tmp, 'link')
  fs.symlinkSync(path.join(tmp, 'real'), rootDir, 'junction')

  expect(await getProjectNodePath({ modulesDir: path.join(rootDir, 'node_modules'), rootDir }, {})).toBeUndefined()
  expect(await getProjectNodePath({ modulesDir: path.join(rootDir, 'vendor'), rootDir }, {})).toBe(path.join(rootDir, 'vendor'))
})

test('a modules directory containing the path-list delimiter is not added to NODE_PATH', async () => {
  const rootDir = temporaryDirectory()
  const modulesDir = path.join(rootDir, `vendor${path.delimiter}other`)
  expect(await getProjectNodePath({ modulesDir, rootDir }, {})).toBeUndefined()
})
