import fs from 'fs'
import os from 'os'
import path from 'path'
import { prepare, preparePackages, tempDir } from '@pnpm/prepare'
import { install } from '@pnpm/plugin-commands-installation'
import { filterPackagesFromDir } from '@pnpm/workspace.filter-packages-from-dir'
import { sync as writeYamlFile } from 'write-yaml-file'
import tempy from 'tempy'
import { patch, patchCommit, patchRemove } from '@pnpm/plugin-commands-patching'
import { readProjectManifest } from '@pnpm/read-project-manifest'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { DEFAULT_OPTS } from './utils/index'
import { fixtures } from '@pnpm/test-fixtures'
import * as enquirer from 'enquirer'

jest.mock('enquirer', () => ({ prompt: jest.fn() }))

// eslint-disable-next-line
const prompt = enquirer.prompt as any
const f = fixtures(__dirname)

const basePatchOption = {
  pnpmHomeDir: '',
  rawConfig: {
    registry: `http://localhost:${REGISTRY_MOCK_PORT}/`,
  },
  registries: { default: `http://localhost:${REGISTRY_MOCK_PORT}/` },
  userConfig: {},
  virtualStoreDir: 'node_modules/.pnpm',
  virtualStoreDirMaxLength: 120,
}

describe('patch and commit', () => {
  let defaultPatchOption: patch.PatchCommandOptions
  let cacheDir: string
  let storeDir: string

  beforeEach(async () => {
    prepare({
      dependencies: {
        'is-positive': '1.0.0',
      },
    })
    cacheDir = path.resolve('cache')
    storeDir = path.resolve('store')
    defaultPatchOption = {
      ...basePatchOption,
      cacheDir,
      dir: process.cwd(),
      storeDir,
    }

    await install.handler({
      ...DEFAULT_OPTS,
      cacheDir,
      storeDir,
      dir: process.cwd(),
      saveLockfile: true,
    })
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
      cacheDir,
      dir: process.cwd(),
      rootProjectManifestDir: process.cwd(),
      frozenLockfile: false,
      fixLockfile: true,
      storeDir,
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

  test('patch and commit with filtered files', async () => {
    const output = await patch.handler(defaultPatchOption, ['is-positive@1.0.0'])
    const patchDir = getPatchDirFromPatchOutput(output)
    const tempDir = os.tmpdir() // temp dir depends on the operating system (@see tempy)

    // store patch files in a temporary directory when not given editDir option
    expect(patchDir).toContain(tempDir)
    expect(fs.existsSync(patchDir)).toBe(true)

    // sanity check to ensure that the license file contains the expected string
    expect(fs.readFileSync(path.join(patchDir, 'license'), 'utf8')).toContain('The MIT License (MIT)')
    fs.writeFileSync(path.join(patchDir, 'ignore.txt'), '', 'utf8')

    const { manifest } = await readProjectManifest(patchDir)
    expect(manifest?.files).toStrictEqual(['index.js'])

    await patchCommit.handler({
      ...DEFAULT_OPTS,
      cacheDir,
      dir: process.cwd(),
      rootProjectManifestDir: process.cwd(),
      frozenLockfile: false,
      fixLockfile: true,
      storeDir,
    }, [patchDir])

    expect(fs.existsSync('node_modules/is-positive/ignore.txt')).toBe(false)
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
      cacheDir,
      dir: process.cwd(),
      rootProjectManifestDir: process.cwd(),
      frozenLockfile: false,
      fixLockfile: true,
      storeDir,
    }, [patchDir])

    expect(fs.readFileSync('node_modules/is-positive/index.js', 'utf8')).toContain('// test patching')
  })

  test('patch and commit with custom patches dir', async () => {
    const patchesDir = 'ts/src/../custom-patches'

    const output = await patch.handler({ ...defaultPatchOption }, ['is-positive@1.0.0'])
    const patchDir = getPatchDirFromPatchOutput(output)

    expect(fs.existsSync(path.normalize(patchesDir))).toBe(false)

    fs.appendFileSync(path.join(patchDir, 'index.js'), '// test patching', 'utf8')

    await patchCommit.handler({
      ...DEFAULT_OPTS,
      cacheDir,
      dir: process.cwd(),
      rootProjectManifestDir: process.cwd(),
      frozenLockfile: false,
      fixLockfile: true,
      patchesDir,
      storeDir,
    }, [patchDir])

    const { manifest } = await readProjectManifest(process.cwd())
    expect(manifest.pnpm?.patchedDependencies).toStrictEqual({
      'is-positive@1.0.0': 'ts/custom-patches/is-positive@1.0.0.patch',
    })
    expect(fs.existsSync(path.normalize(patchesDir))).toBe(true)
    expect(fs.readFileSync(path.join(patchesDir, 'is-positive@1.0.0.patch'), 'utf8')).toContain('// test patching')
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
      cacheDir,
      dir: process.cwd(),
      rootProjectManifestDir: process.cwd(),
      frozenLockfile: false,
      fixLockfile: true,
      storeDir,
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
      cacheDir,
      dir: process.cwd(),
      rootProjectManifestDir: process.cwd(),
      frozenLockfile: false,
      fixLockfile: true,
      storeDir,
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
      cacheDir,
      dir: process.cwd(),
      rootProjectManifestDir: process.cwd(),
      frozenLockfile: false,
      fixLockfile: true,
      storeDir,
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

  test('patch throw an error if no package specified', async () => {
    await expect(() => patch.handler({ ...defaultPatchOption }, []))
      .rejects.toThrow('`pnpm patch` requires the package name')
  })

  test('should throw an error if no installed versions found for patched package', async () => {
    await expect(() => patch.handler(defaultPatchOption, ['chalk']))
      .rejects.toThrow(`Can not find chalk in project ${process.cwd()}, did you forget to install chalk?`)
  })

  test('should throw an error if no preferred versions found for patched package', async () => {
    await expect(() => patch.handler(defaultPatchOption, ['is-positive@2.0.0']))
      .rejects.toThrow(`Can not find is-positive@2.0.0 in project ${process.cwd()}, you can specify currently installed version: 1.0.0.`)
  })

  test('patch package with installed version', async () => {
    const output = await patch.handler(defaultPatchOption, ['is-positive@1'])
    const patchDir = getPatchDirFromPatchOutput(output)
    const tempDir = os.tmpdir()
    expect(patchDir).toContain(tempDir)
    expect(fs.existsSync(patchDir)).toBe(true)
    expect(JSON.parse(fs.readFileSync(path.join(patchDir, 'package.json'), 'utf8')).version).toBe('1.0.0')
  })

  test('should skip empty patch content and not create patches dir', async () => {
    const output = await patch.handler(defaultPatchOption, ['is-positive@1.0.0'])
    const patchDir = getPatchDirFromPatchOutput(output)
    const result = await patchCommit.handler({
      ...DEFAULT_OPTS,
      cacheDir,
      dir: process.cwd(),
      rootProjectManifestDir: process.cwd(),
      frozenLockfile: false,
      fixLockfile: true,
      storeDir,
    }, [patchDir])
    expect(result).toBe(`No changes were found to the following directory: ${patchDir}`)
    expect(fs.existsSync('patches/is-positive@1.0.0.patch')).toBe(false)
    expect(fs.existsSync('patches')).toBe(false)
  })
})

describe('prompt to choose version', () => {
  let defaultPatchOption: patch.PatchCommandOptions
  let cacheDir: string
  let storeDir: string
  beforeEach(() => {
    prepare({
      dependencies: {
        ava: '5.2.0',
        chalk: '4.1.2',
      },
    })
    cacheDir = path.resolve('cache')
    storeDir = path.resolve('store')
    defaultPatchOption = {
      ...basePatchOption,
      cacheDir,
      dir: process.cwd(),
      storeDir,
    }
  })

  test('prompt to choose version if multiple version founded for patched package', async () => {
    await install.handler({
      ...DEFAULT_OPTS,
      cacheDir,
      storeDir,
      dir: process.cwd(),
      saveLockfile: true,
    })
    prompt.mockResolvedValue({
      version: '5.3.0',
    })
    prompt.mockClear()
    const output = await patch.handler(defaultPatchOption, ['chalk'])

    expect(prompt.mock.calls[0][0].choices).toEqual(expect.arrayContaining([
      {
        name: '4.1.2',
        message: '4.1.2',
        value: '4.1.2',
      },
      {
        name: '5.3.0',
        message: '5.3.0',
        value: '5.3.0',
      },
    ]))

    const patchDir = getPatchDirFromPatchOutput(output)
    const tempDir = os.tmpdir()

    expect(patchDir).toContain(tempDir)
    expect(fs.existsSync(patchDir)).toBe(true)
    expect(JSON.parse(fs.readFileSync(path.join(patchDir, 'package.json'), 'utf8')).version).toBe('5.3.0')
    expect(fs.existsSync(path.join(patchDir, 'source/index.js'))).toBe(true)

    fs.appendFileSync(path.join(patchDir, 'source/index.js'), '// test patching', 'utf8')
    await patchCommit.handler({
      ...DEFAULT_OPTS,
      cacheDir,
      dir: process.cwd(),
      rootProjectManifestDir: process.cwd(),
      frozenLockfile: false,
      fixLockfile: true,
      storeDir,
    }, [patchDir])

    const { manifest } = await readProjectManifest(process.cwd())
    expect(manifest.pnpm?.patchedDependencies).toStrictEqual({
      'chalk@5.3.0': 'patches/chalk@5.3.0.patch',
    })
    const patchContent = fs.readFileSync('patches/chalk@5.3.0.patch', 'utf8')
    expect(patchContent).toContain('diff --git')
    expect(patchContent).toContain('// test patching')
    expect(fs.readFileSync('node_modules/.pnpm/ava@5.2.0/node_modules/chalk/source/index.js', 'utf8')).toContain('// test patching')
  })
})

describe('patching should work when there is a no EOL in the patched file', () => {
  let defaultPatchOption: patch.PatchCommandOptions

  beforeEach(async () => {
    prepare({
      dependencies: {
        'safe-execa': '0.1.2',
      },
    })

    const cacheDir = path.resolve('cache')
    const storeDir = path.resolve('store')

    defaultPatchOption = {
      ...basePatchOption,
      cacheDir,
      dir: process.cwd(),
      storeDir,
    }

    await install.handler({
      ...DEFAULT_OPTS,
      cacheDir,
      storeDir,
      dir: process.cwd(),
      saveLockfile: true,
    })
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
      rootProjectManifestDir: process.cwd(),
      frozenLockfile: false,
      fixLockfile: true,
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
      rootProjectManifestDir: process.cwd(),
      frozenLockfile: false,
      fixLockfile: true,
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

  beforeEach(async () => {
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
          hi: '0.0.0',
        },
      },
      {
        name: 'project-2',
        version: '1.0.0',
        dependencies: {
          'is-positive': '1.0.0',
          'project-1': '1',
          hi: 'github:zkochan/hi#4cdebec76b7b9d1f6e219e06c42d92a6b8ea60cd',
        },
      },
    ])

    cacheDir = path.resolve('cache')
    storeDir = path.resolve('store')

    defaultPatchOption = {
      ...basePatchOption,
      cacheDir,
      dir: process.cwd(),
      storeDir,
    }
    writeYamlFile('pnpm-workspace.yaml', { packages: ['project-1', 'project-2'] })
  })

  test('patch commit should work in workspaces', async () => {
    const { allProjects, allProjectsGraph, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [])
    await install.handler({
      ...DEFAULT_OPTS,
      cacheDir,
      storeDir,
      allProjects,
      allProjectsGraph,
      dir: process.cwd(),
      lockfileDir: process.cwd(),
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
      rootProjectManifestDir: process.cwd(),
      cacheDir,
      storeDir,
      lockfileDir: process.cwd(),
      workspaceDir: process.cwd(),
      saveLockfile: true,
      frozenLockfile: false,
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

  test('patch and patch-commit should work with shared-workspace-lockfile=false', async () => {
    const { allProjects, allProjectsGraph, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [])
    await install.handler({
      ...DEFAULT_OPTS,
      cacheDir,
      storeDir,
      allProjects,
      allProjectsGraph,
      dir: process.cwd(),
      lockfileDir: undefined,
      selectedProjectsGraph,
      workspaceDir: process.cwd(),
      saveLockfile: true,
      sharedWorkspaceLockfile: false,
    })
    if (path.basename(process.cwd()) !== 'project-1') {
      process.chdir('./project-1')
    }
    const output = await patch.handler({
      ...defaultPatchOption,
      dir: process.cwd(),
    }, ['is-positive@1.0.0'])
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
      rootProjectManifestDir: process.cwd(),
      cacheDir,
      storeDir,
      lockfileDir: process.cwd(),
      workspaceDir: process.cwd(),
      saveLockfile: true,
      frozenLockfile: false,
      fixLockfile: true,
      sharedWorkspaceLockfile: false,
    }, [patchDir])

    const { manifest } = await readProjectManifest(process.cwd())
    expect(manifest.pnpm?.patchedDependencies).toStrictEqual({
      'is-positive@1.0.0': 'patches/is-positive@1.0.0.patch',
    })
    const patchContent = fs.readFileSync('patches/is-positive@1.0.0.patch', 'utf8')
    expect(patchContent).toContain('diff --git')
    expect(patchContent).toContain('// test patching')
    expect(fs.readFileSync('./node_modules/is-positive/index.js', 'utf8')).toContain('// test patching')
    expect(fs.existsSync('./node_modules/is-positive/license')).toBe(false)
    expect(fs.readFileSync('../project-2/node_modules/is-positive/index.js', 'utf8')).not.toContain('// test patching')
    expect(fs.existsSync('../project-2/node_modules/is-positive/license')).toBe(true)
  })

  test('reusing existing patch file should work with shared-workspace-lockfile=false', async () => {
    const { allProjects, allProjectsGraph, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [])
    await install.handler({
      ...DEFAULT_OPTS,
      cacheDir,
      storeDir,
      allProjects,
      allProjectsGraph,
      dir: process.cwd(),
      lockfileDir: undefined,
      selectedProjectsGraph,
      workspaceDir: process.cwd(),
      saveLockfile: true,
      sharedWorkspaceLockfile: false,
    })

    // patch project-1
    process.chdir('./project-1')
    let output = await patch.handler({
      ...defaultPatchOption,
      dir: process.cwd(),
    }, ['is-positive@1.0.0'])
    let patchDir = getPatchDirFromPatchOutput(output)

    // modify index.js and remove license
    fs.appendFileSync(path.join(patchDir, 'index.js'), '// test patching', 'utf8')
    fs.unlinkSync(path.join(patchDir, 'license'))

    // patch-commit
    await patchCommit.handler({
      ...DEFAULT_OPTS,
      allProjects,
      allProjectsGraph,
      selectedProjectsGraph,
      dir: process.cwd(),
      rootProjectManifestDir: process.cwd(),
      cacheDir,
      storeDir,
      lockfileDir: process.cwd(),
      workspaceDir: process.cwd(),
      saveLockfile: true,
      frozenLockfile: false,
      fixLockfile: true,
      sharedWorkspaceLockfile: false,
    }, [patchDir])

    // verify committed patch
    expect(fs.readFileSync('./node_modules/is-positive/index.js', 'utf8')).toContain('// test patching')
    expect(fs.existsSync('./node_modules/is-positive/license')).toBe(false)

    // re-patch project-1
    output = await patch.handler({
      ...defaultPatchOption,
      dir: process.cwd(),
    }, ['is-positive@1.0.0'])
    patchDir = getPatchDirFromPatchOutput(output)
    expect(fs.existsSync(patchDir)).toBe(true)

    // verify temporary patch is reusing last committed patch
    expect(fs.readFileSync(path.join(patchDir, 'index.js'), 'utf8')).toContain('// test patching')
    expect(fs.existsSync(path.join(patchDir, 'license'))).toBe(false)
  })

  test('patch and patch-commit for git hosted dependency', async () => {
    const { allProjects, allProjectsGraph, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [])
    await install.handler({
      ...DEFAULT_OPTS,
      cacheDir,
      storeDir,
      allProjects,
      allProjectsGraph,
      dir: process.cwd(),
      lockfileDir: process.cwd(),
      selectedProjectsGraph,
      workspaceDir: process.cwd(),
      saveLockfile: true,
    })

    prompt.mockResolvedValue({
      version: 'https://codeload.github.com/zkochan/hi/tar.gz/4cdebec76b7b9d1f6e219e06c42d92a6b8ea60cd',
    })
    prompt.mockClear()
    const output = await patch.handler(defaultPatchOption, ['hi'])
    expect(prompt.mock.calls[0][0].choices).toEqual(expect.arrayContaining([
      {
        name: '0.0.0',
        message: '0.0.0',
        value: '0.0.0',
      },
      {
        name: '1.0.0',
        message: '1.0.0',
        value: 'https://codeload.github.com/zkochan/hi/tar.gz/4cdebec76b7b9d1f6e219e06c42d92a6b8ea60cd',
        hint: 'Git Hosted',
      },
    ]))
    const patchDir = getPatchDirFromPatchOutput(output)
    expect(fs.existsSync(patchDir)).toBe(true)
    expect(fs.readFileSync(path.join(patchDir, 'index.js'), 'utf8')).toContain('module.exports = \'Hi\'')
    fs.appendFileSync(path.join(patchDir, 'index.js'), '// test patching', 'utf8')

    await patchCommit.handler({
      ...DEFAULT_OPTS,
      allProjects,
      allProjectsGraph,
      selectedProjectsGraph,
      dir: process.cwd(),
      rootProjectManifestDir: process.cwd(),
      cacheDir,
      storeDir,
      lockfileDir: process.cwd(),
      workspaceDir: process.cwd(),
      saveLockfile: true,
      frozenLockfile: false,
    }, [patchDir])

    const { manifest } = await readProjectManifest(process.cwd())
    expect(manifest.pnpm?.patchedDependencies).toStrictEqual({
      'hi@1.0.0': 'patches/hi@1.0.0.patch',
    })
    const patchContent = fs.readFileSync('patches/hi@1.0.0.patch', 'utf8')
    expect(patchContent).toContain('diff --git')
    expect(patchContent).toContain('// test patching')
    expect(fs.readFileSync('./project-2/node_modules/hi/index.js', 'utf8')).toContain('// test patching')
  })
})

describe('patch with custom modules-dir and virtual-store-dir', () => {
  let defaultPatchOption: patch.PatchCommandOptions
  let customModulesDirFixture: string
  let cacheDir: string
  let storeDir: string
  beforeAll(() => {
    customModulesDirFixture = tempDir()
    f.copy('custom-modules-dir', customModulesDirFixture)
    cacheDir = path.resolve(customModulesDirFixture, 'cache')
    storeDir = path.resolve(customModulesDirFixture, 'store')
    defaultPatchOption = {
      ...basePatchOption,
      cacheDir,
      dir: customModulesDirFixture,
      storeDir,
      modulesDir: 'fake_modules',
      virtualStoreDir: 'fake_modules/.fake_store',
    }
  })

  test('should work with custom modules-dir and virtual-store-dir', async () => {
    const manifest = fs.readFileSync(path.join(customModulesDirFixture, 'package.json'), 'utf8')
    const lockfileYaml = fs.readFileSync(path.join(customModulesDirFixture, 'pnpm-lock.yaml'), 'utf8')
    const { allProjects, allProjectsGraph, selectedProjectsGraph } = await filterPackagesFromDir(customModulesDirFixture, [])
    await install.handler({
      ...DEFAULT_OPTS,
      cacheDir,
      storeDir,
      dir: customModulesDirFixture,
      lockfileDir: customModulesDirFixture,
      allProjects,
      allProjectsGraph,
      selectedProjectsGraph,
      workspaceDir: customModulesDirFixture,
      saveLockfile: true,
      modulesDir: 'fake_modules',
      virtualStoreDir: 'fake_modules/.fake_store',
    })
    const output = await patch.handler(defaultPatchOption, ['is-positive@1'])
    const patchDir = getPatchDirFromPatchOutput(output)
    const tempDir = os.tmpdir()
    expect(patchDir).toContain(tempDir)
    expect(fs.existsSync(patchDir)).toBe(true)
    expect(JSON.parse(fs.readFileSync(path.join(patchDir, 'package.json'), 'utf8')).version).toBe('1.0.0')

    fs.appendFileSync(path.join(patchDir, 'index.js'), '// test patching', 'utf8')

    await patchCommit.handler({
      ...DEFAULT_OPTS,
      dir: customModulesDirFixture,
      rootProjectManifestDir: customModulesDirFixture,
      saveLockfile: true,
      frozenLockfile: false,
      fixLockfile: true,
      allProjects,
      allProjectsGraph,
      selectedProjectsGraph,
      modulesDir: 'fake_modules',
      virtualStoreDir: 'fake_modules/.fake_store',
      lockfileDir: customModulesDirFixture,
      workspaceDir: customModulesDirFixture,
    }, [patchDir])
    expect(fs.readFileSync(path.join(customModulesDirFixture, 'packages/bar/fake_modules/is-positive/index.js'), 'utf8')).toContain('// test patching')
    // restore package.json and package-lock.yaml
    fs.writeFileSync(path.join(customModulesDirFixture, 'package.json'), manifest, 'utf8')
    fs.writeFileSync(path.join(customModulesDirFixture, 'pnpm-lock.yaml'), lockfileYaml, 'utf8')
  })
})

describe('patch-remove', () => {
  let defaultPatchRemoveOption: patchRemove.PatchRemoveCommandOptions
  let cacheDir: string
  let storeDir: string

  beforeEach(async () => {
    prompt.mockClear()
    prepare({
      dependencies: {
        'is-positive': '1.0.0',
      },
    })
    cacheDir = path.resolve('cache')
    storeDir = path.resolve('store')
    defaultPatchRemoveOption = {
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    }

    await install.handler({
      ...DEFAULT_OPTS,
      cacheDir,
      storeDir,
      dir: process.cwd(),
      saveLockfile: true,
    })
  })
  test('patch-remove should work as expected', async () => {
    const { manifest, writeProjectManifest } = await readProjectManifest(process.cwd())
    manifest.pnpm = {
      patchedDependencies: {
        'is-positive@1.0.0': 'patches/is-positive@1.0.0.patch',
      },
    }
    await writeProjectManifest(manifest)
    fs.mkdirSync(path.join(process.cwd(), 'patches'))
    fs.writeFileSync(path.join(process.cwd(), 'patches/is-positive@1.0.0.patch'), 'test patch content', 'utf8')

    await patchRemove.handler(defaultPatchRemoveOption, ['is-positive@1.0.0'])

    const { manifest: newManifest } = await readProjectManifest(process.cwd())
    expect(newManifest!.pnpm!).toBeUndefined()
    expect(fs.existsSync(path.join(process.cwd(), 'patches/is-positive@1.0.0.patch'))).toBe(false)
    expect(fs.existsSync(path.join(process.cwd(), 'patches'))).toBe(false)
  })

  test('prompt to select patches that to be removed', async () => {
    const { manifest, writeProjectManifest } = await readProjectManifest(process.cwd())
    manifest.pnpm = {
      patchedDependencies: {
        'is-positive@1.0.0': 'patches/is-positive@1.0.0.patch',
        'chalk@4.1.2': 'patches/chalk@4.1.2.patch',
      },
    }
    await writeProjectManifest(manifest)
    prompt.mockResolvedValue({
      patches: ['is-positive@1.0.0', 'chalk@4.1.2'],
    })
    await patchRemove.handler(defaultPatchRemoveOption, [])
    expect(prompt.mock.calls[0][0].choices).toEqual(expect.arrayContaining(['is-positive@1.0.0', 'chalk@4.1.2']))
    prompt.mockClear()

    const { manifest: newManifest } = await readProjectManifest(process.cwd())
    expect(newManifest!.pnpm!).toBeUndefined()
  })

  test('should throw error when there is no patch to remove', async () => {
    await expect(() => patchRemove.handler(defaultPatchRemoveOption, []))
      .rejects.toThrow('There are no patches that need to be removed')
  })
})

function getPatchDirFromPatchOutput (output: string): string {
  const match = output.match(/'([^']+)'/)
  if (match?.[1] == null) throw new Error('No path in output')
  return match[1]
}
