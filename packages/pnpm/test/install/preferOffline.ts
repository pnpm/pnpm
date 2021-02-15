import { WANTED_LOCKFILE } from '@pnpm/constants'
import prepare from '@pnpm/prepare'
import rimraf from '@zkochan/rimraf'
import {
  addDistTag,
  execPnpm,
} from '../utils'

test('when prefer offline is used, meta from store is used, where latest might be out-of-date', async () => {
  const project = prepare()

  await addDistTag('foo', '100.0.0', 'latest')

  // This will cache the meta of `foo`
  await execPnpm(['install', 'foo'])

  await rimraf('node_modules')
  await rimraf(WANTED_LOCKFILE)

  await addDistTag('foo', '100.1.0', 'latest')

  await execPnpm(['install', 'foo', '--prefer-offline'])

  expect(project.requireModule('foo/package.json').version).toBe('100.0.0')
})
