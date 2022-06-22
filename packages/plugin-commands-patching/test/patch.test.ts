import fs from 'fs'
import path from 'path'
import prepare from '@pnpm/prepare'
import { patch, patchCommit } from '@pnpm/plugin-commands-patching'
import readProjectManifest from '@pnpm/read-project-manifest'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { DEFAULT_OPTS } from './utils/index'

test('patch and commit', async () => {
  prepare({
    dependencies: {
      'is-positive': '1.0.0',
    },
  })
  const cacheDir = path.resolve('cache')
  const storeDir = path.resolve('store')

  const output = await patch.handler({
    cacheDir,
    dir: process.cwd(),
    pnpmHomeDir: '',
    rawConfig: {
      registry: `http://localhost:${REGISTRY_MOCK_PORT}/`,
    },
    registries: { default: `http://localhost:${REGISTRY_MOCK_PORT}/` },
    storeDir,
    userConfig: {},
  }, ['is-positive@1.0.0'])

  const userPatchDir = output.substring(output.indexOf(':') + 1).trim()
  fs.appendFileSync(path.join(userPatchDir, 'index.js'), '// test patching', 'utf8')

  await patchCommit.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  }, [userPatchDir])

  const { manifest } = await readProjectManifest(process.cwd())
  expect(manifest.pnpm?.patchedDependencies).toStrictEqual({
    'is-positive@1.0.0': 'patches/is-positive@1.0.0.patch',
  })
  const patchContent = fs.readFileSync('patches/is-positive@1.0.0.patch', 'utf8')
  expect(patchContent).toContain('diff --git')
  expect(patchContent).toContain('// test patching')
  expect(fs.readFileSync('node_modules/is-positive/index.js', 'utf8')).toContain('// test patching')
})
