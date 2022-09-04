import fs from 'fs'
import os from 'os'
import path from 'path'
import prepare from '@pnpm/prepare'
import tempy from 'tempy'
import { patch, patchCommit } from '@pnpm/plugin-commands-patching'
import readProjectManifest from '@pnpm/read-project-manifest'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { DEFAULT_OPTS } from './utils/index'

describe('patch and commit', () => {
  let defaultPatchOption: patch.PatchCommandOptions
  const tempySpy = jest.spyOn(tempy, 'directory')

  beforeEach(() => {
    prepare({
      dependencies: {
        'is-positive': '1.0.0',
      },
    })

    const cacheDir = path.resolve('cache')
    const storeDir = path.resolve('store')

    defaultPatchOption = {
      cacheDir,
      dir: process.cwd(),
      pnpmHomeDir: '',
      rawConfig: {
        registry: `http://localhost:${REGISTRY_MOCK_PORT}/`,
      },
      registries: { default: `http://localhost:${REGISTRY_MOCK_PORT}/` },
      storeDir,
      userConfig: {},
    }
  })

  test('patch and commit', async () => {
    const output = await patch.handler(defaultPatchOption, ['is-positive@1.0.0'])
    const userPatchDir = output.substring(output.indexOf(':') + 1).trim()
    const tempDir = os.tmpdir() // temp dir depends on the operating system (@see tempy)

    // store patch files(user, source) in temporary directory when not given editDir option
    expect(userPatchDir).toContain(tempDir)
    expect(fs.existsSync(userPatchDir)).toBe(true)
    expect(fs.existsSync(userPatchDir.replace('/user', '/source'))).toBe(true)

    // sanity check to ensure that the license file contains the expected string
    expect(fs.readFileSync(path.join(userPatchDir, 'license'), 'utf8')).toContain('The MIT License (MIT)')

    fs.appendFileSync(path.join(userPatchDir, 'index.js'), '// test patching', 'utf8')
    fs.unlinkSync(path.join(userPatchDir, 'license'))

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

    expect(patchContent).not.toContain('The MIT License (MIT)')
    expect(fs.existsSync('node_modules/is-positive/license')).toBe(false)
  })

  test('store source files in temporary directory and user files in user directory, when given editDir option', async () => {
    const editDir = 'test/user/is-positive'

    const patchFn = async () => patch.handler({ ...defaultPatchOption, editDir }, ['is-positive@1.0.0'])
    const output = await patchFn()
    const userPatchDir = output.substring(output.indexOf(':') + 1).trim()

    expect(userPatchDir).toBe(editDir)
    expect(fs.existsSync(userPatchDir)).toBe(true)
    expect(fs.existsSync(path.join(tempySpy.mock.results[0].value, '/source'))).toBe(true)

    // If editDir already exists, it should throw an error
    await expect(patchFn()).rejects.toThrow(`The target directory already exists: '${editDir}'`)
  })
})
