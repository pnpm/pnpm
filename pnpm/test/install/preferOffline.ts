import { WANTED_LOCKFILE } from '@pnpm/constants'
import { prepare } from '@pnpm/prepare'
import { sync as rimraf } from '@zkochan/rimraf'
import {
  addDistTag,
  execPnpm,
} from '../utils'

test('when prefer offline is used, meta from store is used, where latest might be out-of-date', async () => {
  const project = prepare()

  await addDistTag('@pnpm.e2e/foo', '100.0.0', 'latest')

  // This will cache the meta of `foo`
  await execPnpm(['install', '@pnpm.e2e/foo'])

  rimraf('node_modules')
  rimraf(WANTED_LOCKFILE)

  await addDistTag('@pnpm.e2e/foo', '100.1.0', 'latest')

  await execPnpm(['install', '@pnpm.e2e/foo', '--prefer-offline'])

  expect(project.requireModule('@pnpm.e2e/foo/package.json').version).toBe('100.0.0')
})
