import fs from 'fs'
import path from 'path'
import prepare from '@pnpm/prepare'
import { patch, patchCommit } from '@pnpm/plugin-commands-patching'
import readProjectManifest from '@pnpm/read-project-manifest'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'

test('patch and commit', async () => {
  prepare()
  const cacheDir = path.resolve('cache')
  const storeDir = path.resolve('store')

  await patch.handler({
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

  const tempDir = path.join(storeDir, 'v3/tmp')
  const [patchDir] = fs.readdirSync(tempDir)
  const userPatchDir = path.join(tempDir, patchDir, 'user')
  fs.appendFileSync(path.join(userPatchDir, 'index.js'), '// test patching', 'utf8')

  await patchCommit.handler({
    dir: process.cwd(),
  }, [userPatchDir])

  const { manifest } = await readProjectManifest(process.cwd())
  expect(manifest.pnpm?.patchedDependencies).toStrictEqual({
    'is-positive@1.0.0': 'patches/is-positive@1.0.0.patch',
  })
  const patchContent = fs.readFileSync('patches/is-positive@1.0.0.patch', 'utf8')
  expect(patchContent).toContain('diff --git')
  expect(patchContent).toContain('// test patching')
})
