import { MANIFEST_BASE_NAMES } from '@pnpm/constants'
import { prepareEmpty } from '@pnpm/prepare'
import { writeProjectManifest } from '@pnpm/write-project-manifest'
import { statManifestFile } from '../src/statManifestFile'

test.each(MANIFEST_BASE_NAMES)('load %s', async baseName => {
  prepareEmpty()
  await writeProjectManifest(`foo/bar/${baseName}`, { name: 'foo', version: '1.0.0' })
  const stats = await statManifestFile('foo/bar')
  expect(stats).toBeDefined()
  expect(stats?.isFile()).toBe(true)
})

test('should return undefined if no manifest is found', async () => {
  prepareEmpty()
  expect(await statManifestFile('.')).toBeUndefined()
})
