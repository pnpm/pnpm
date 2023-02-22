import fs from 'fs'
import os from 'os'
import path from 'path'
import { prepare, preparePackages } from '@pnpm/prepare'
import { install } from '@pnpm/plugin-commands-installation'
import { readProjects } from '@pnpm/filter-workspace-packages'
import writeYamlFile from 'write-yaml-file'
import tempy from 'tempy'
import { patch, patchCommit } from '@pnpm/plugin-commands-patching'
import { readProjectManifest } from '@pnpm/read-project-manifest'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { DEFAULT_OPTS } from './utils/index'

describe('patch and commit', () => {
  let defaultPatchOption: patch.PatchCommandOptions

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
    const patchDir = getPatchDirFromPatchOutput(output)
    const tempDir = os.tmpdir() // temp dir depends on the operating system (@see tempy)

    // store patch files in a temporary directory when not given editDir option
    expect(patchDir).toContain(tempDir)
    expect(fs.existsSync(patchDir)).toBe(true)

    // sanity check to ensure that the license file contains the expected string
    expect(fs.readFileSync(path.join(patchDir, 'license'), 'utf8')).toContain('The MIT License (MIT)')

    fs.appendFileSync(path.join(patchDir, 'index.js'), '// test patching', 'utf8')
    fs.unlinkSync(path.join(patchDir, 'license'))

    await patchCommit.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    }, [patchDir])

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

  test('patch and commit with a custom edit dir', async () => {
    const editDir = path.join(tempy.directory())

    const output = await patch.handler({ ...defaultPatchOption, editDir }, ['is-positive@1.0.0'])
    const patchDir = getPatchDirFromPatchOutput(output)

    expect(patchDir).toBe(editDir)
    expect(fs.existsSync(patchDir)).toBe(true)

    fs.appendFileSync(path.join(patchDir, 'index.js'), '// test patching', 'utf8')

    await patchCommit.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    }, [patchDir])

    expect(fs.readFileSync('node_modules/is-positive/index.js', 'utf8')).toContain('// test patching')
  })

  test('patch throws an error if the edit-dir already exists and is not empty', async () => {
    const editDir = tempy.directory()
    fs.writeFileSync(path.join(editDir, 'test.txt'), '', 'utf8')

    await expect(() => patch.handler({ ...defaultPatchOption, editDir }, ['is-positive@1.0.0']))
      .rejects.toThrow(`The target directory already exists: '${editDir}'`)
  })

  test('patch and commit should work when the patch directory is specified with a trailing slash', async () => {
    const editDir = path.join(tempy.directory()) + (os.platform() === 'win32' ? '\\' : '/')

    const output = await patch.handler({ ...defaultPatchOption, editDir }, ['is-positive@1.0.0'])
    const patchDir = getPatchDirFromPatchOutput(output)

    expect(patchDir).toBe(editDir)
    expect(fs.existsSync(patchDir)).toBe(true)

    fs.appendFileSync(path.join(patchDir, 'index.js'), '// test patching', 'utf8')

    await patchCommit.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    }, [patchDir])

    expect(fs.readFileSync('node_modules/is-positive/index.js', 'utf8')).toContain('// test patching')
  })

  test('should reuse existing patch file by default', async () => {
    let output = await patch.handler(defaultPatchOption, ['is-positive@1.0.0'])
    let patchDir = getPatchDirFromPatchOutput(output)

    fs.appendFileSync(path.join(patchDir, 'index.js'), '// test patching', 'utf8')
    fs.unlinkSync(path.join(patchDir, 'license'))

    await patchCommit.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    }, [patchDir])

    const { manifest } = await readProjectManifest(process.cwd())
    expect(manifest.pnpm?.patchedDependencies).toStrictEqual({
      'is-positive@1.0.0': 'patches/is-positive@1.0.0.patch',
    })
    expect(fs.existsSync('patches/is-positive@1.0.0.patch')).toBe(true)

    // re-patch
    output = await patch.handler({ ...defaultPatchOption, rootProjectManifest: manifest }, ['is-positive@1.0.0'])
    patchDir = getPatchDirFromPatchOutput(output)

    expect(fs.existsSync(patchDir)).toBe(true)
    expect(fs.existsSync(path.join(patchDir, 'license'))).toBe(false)
    expect(fs.readFileSync(path.join(patchDir, 'index.js'), 'utf8')).toContain('// test patching')
  })

  test('if the patch file is not existed when patching, should throw an error', async () => {
    const { writeProjectManifest, manifest } = await readProjectManifest(process.cwd())
    await writeProjectManifest({
      ...manifest,
      pnpm: {
        patchedDependencies: {
          'is-positive@1.0.0': 'patches/not-found.patch',
        },
      },
    })

    try {
      await patch.handler(defaultPatchOption, ['is-positive@1.0.0'])
    } catch (err: any) { // eslint-disable-line
      expect(err.code).toBe('ERR_PNPM_PATCH_FILE_NOT_FOUND')
    }
  })

  test('should ignore patch files with --ignore-patches', async () => {
    let output = await patch.handler(defaultPatchOption, ['is-positive@1.0.0'])
    let patchDir = getPatchDirFromPatchOutput(output)

    fs.appendFileSync(path.join(patchDir, 'index.js'), '// test patching', 'utf8')
    fs.unlinkSync(path.join(patchDir, 'license'))

    await patchCommit.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    }, [patchDir])

    const { manifest } = await readProjectManifest(process.cwd())
    expect(manifest.pnpm?.patchedDependencies).toStrictEqual({
      'is-positive@1.0.0': 'patches/is-positive@1.0.0.patch',
    })
    expect(fs.existsSync('patches/is-positive@1.0.0.patch')).toBe(true)

    // re-patch with --ignore-patches
    output = await patch.handler({ ...defaultPatchOption, ignoreExisting: true }, ['is-positive@1.0.0'])
    patchDir = getPatchDirFromPatchOutput(output)

    expect(fs.existsSync(patchDir)).toBe(true)
    expect(fs.existsSync(path.join(patchDir, 'license'))).toBe(true)
    expect(fs.readFileSync(path.join(patchDir, 'index.js'), 'utf8')).not.toContain('// test patching')
  })
})

describe('patching should work when there is a no EOL in the patched file', () => {
  let defaultPatchOption: patch.PatchCommandOptions

  beforeEach(() => {
    prepare({
      dependencies: {
        'safe-execa': '0.1.2',
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
  it('should work when adding content on a newline', async () => {
    const output = await patch.handler(defaultPatchOption, ['safe-execa@0.1.2'])
    const userPatchDir = getPatchDirFromPatchOutput(output)
    const tempDir = os.tmpdir()

    expect(userPatchDir).toContain(tempDir)
    expect(fs.existsSync(userPatchDir)).toBe(true)
    expect(fs.existsSync(path.join(userPatchDir, 'lib/index.js'))).toBe(true)

    fs.appendFileSync(path.join(userPatchDir, 'lib/index.js'), '\n// test patching', 'utf8')

    await patchCommit.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    }, [userPatchDir])

    const { manifest } = await readProjectManifest(process.cwd())
    expect(manifest.pnpm?.patchedDependencies).toStrictEqual({
      'safe-execa@0.1.2': 'patches/safe-execa@0.1.2.patch',
    })
    const patchContent = fs.readFileSync('patches/safe-execa@0.1.2.patch', 'utf8')
    expect(patchContent).toContain('diff --git')
    expect(patchContent).toContain('// test patching')
    expect(patchContent).not.toContain('No newline at end of file')
    expect(fs.readFileSync('node_modules/safe-execa/lib/index.js', 'utf8')).toContain('// test patching')
  })
  it('should work fine when new content is appended', async () => {
    const output = await patch.handler(defaultPatchOption, ['safe-execa@0.1.2'])
    const userPatchDir = getPatchDirFromPatchOutput(output)
    const tempDir = os.tmpdir()

    expect(userPatchDir).toContain(tempDir)
    expect(fs.existsSync(userPatchDir)).toBe(true)
    expect(fs.existsSync(path.join(userPatchDir, 'lib/index.js'))).toBe(true)

    fs.appendFileSync(path.join(userPatchDir, 'lib/index.js'), '// patch without newline', 'utf8')

    await patchCommit.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    }, [userPatchDir])

    const { manifest } = await readProjectManifest(process.cwd())
    expect(manifest.pnpm?.patchedDependencies).toStrictEqual({
      'safe-execa@0.1.2': 'patches/safe-execa@0.1.2.patch',
    })
    const patchContent = fs.readFileSync('patches/safe-execa@0.1.2.patch', 'utf8')
    expect(patchContent).toContain('No newline at end of file')
    expect(fs.readFileSync('node_modules/safe-execa/lib/index.js', 'utf8')).toContain('//# sourceMappingURL=index.js.map// patch without newline')
  })
})

describe('patch and commit in workspaces', () => {
  let defaultPatchOption: patch.PatchCommandOptions
  let cacheDir: string
  let storeDir: string

  beforeEach(() => {
    preparePackages([
      {
        location: '.',
        package: {
          name: 'patch-commit-workspaces',
        },
      },
      {
        name: 'project-1',
        version: '1.0.0',
        dependencies: {
          'is-positive': '1.0.0',
        },
      },
      {
        name: 'project-2',
        version: '1.0.0',
        dependencies: {
          'is-positive': '1.0.0',
          'project-1': '1',
        },
      },
    ])

    cacheDir = path.resolve('cache')
    storeDir = path.resolve('store')

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

  test('patch commit should work in workspaces', async () => {
    await writeYamlFile('pnpm-workspace.yaml', { packages: ['project-1', 'project-2'] })
    const { allProjects, allProjectsGraph, selectedProjectsGraph } = await readProjects(process.cwd(), [])
    await install.handler({
      ...DEFAULT_OPTS,
      cacheDir,
      storeDir,
      allProjects,
      allProjectsGraph,
      dir: process.cwd(),
      selectedProjectsGraph,
      workspaceDir: process.cwd(),
      saveLockfile: true,
    })

    const output = await patch.handler(defaultPatchOption, ['is-positive@1.0.0'])
    const patchDir = getPatchDirFromPatchOutput(output)
    const tempDir = os.tmpdir()

    expect(patchDir).toContain(tempDir)
    expect(fs.existsSync(patchDir)).toBe(true)

    expect(fs.readFileSync(path.join(patchDir, 'license'), 'utf8')).toContain('The MIT License (MIT)')

    fs.appendFileSync(path.join(patchDir, 'index.js'), '// test patching', 'utf8')
    fs.unlinkSync(path.join(patchDir, 'license'))

    await patchCommit.handler({
      ...DEFAULT_OPTS,
      allProjects,
      allProjectsGraph,
      selectedProjectsGraph,
      dir: process.cwd(),
      cacheDir,
      storeDir,
      lockfileDir: process.cwd(),
      workspaceDir: process.cwd(),
      saveLockfile: true,
    }, [patchDir])

    const { manifest } = await readProjectManifest(process.cwd())
    expect(manifest.pnpm?.patchedDependencies).toStrictEqual({
      'is-positive@1.0.0': 'patches/is-positive@1.0.0.patch',
    })
    const patchContent = fs.readFileSync('patches/is-positive@1.0.0.patch', 'utf8')
    expect(patchContent).toContain('diff --git')
    expect(patchContent).toContain('// test patching')
    expect(fs.readFileSync('project-1/node_modules/is-positive/index.js', 'utf8')).toContain('// test patching')
    expect(fs.existsSync('project-1/node_modules/is-positive/license')).toBe(false)
    expect(fs.readFileSync('project-2/node_modules/is-positive/index.js', 'utf8')).toContain('// test patching')
    expect(fs.existsSync('project-2/node_modules/is-positive/license')).toBe(false)
  })
})

function getPatchDirFromPatchOutput (output: string) {
  const [firstLine] = output.split('\n')
  return firstLine.substring(firstLine.indexOf(':') + 1).trim()
}
