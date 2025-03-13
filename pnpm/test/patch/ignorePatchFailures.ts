import fs from 'fs'
import { preparePackages } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'
import { sync as writeYamlFile } from 'write-yaml-file'
import { execPnpmSync } from '../utils'

const f = fixtures(__dirname)

describe('ignorePatchFailures=undefined (necessary for backward-compatibility)', () => {
  test('errors on exact version patch failures', async () => {
    preparePackages([
      {
        name: 'foo',
        version: '0.0.0',
        private: true,
        dependencies: {
          'is-positive': '1.0.0', // is-positive@1.0.0.patch should succeed
        },
      },
      {
        name: 'bar',
        version: '0.0.0',
        private: true,
        dependencies: {
          'is-positive': '3.1.0', // is-positive@1.0.0.patch should fail
        },
      },
    ])

    const patchFile = f.find('patch-pkg/is-positive@1.0.0.patch')

    writeYamlFile('pnpm-workspace.yaml', {
      packages: ['**', '!store/**'],
      patchedDependencies: {
        'is-positive@1.0.0': patchFile, // should succeed
        'is-positive@3.1.0': patchFile, // should fail
      },
    })

    // pnpm install should fail
    const { status, stdout } = execPnpmSync(['install'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_PATCH_FAILED')
    const errorLines = stdout.toString().split('\n').filter(line => line.includes('ERR_PNPM_PATCH_FAILED'))
    expect(errorLines).toStrictEqual([expect.stringContaining(patchFile)])
    expect(errorLines).toStrictEqual([expect.stringContaining('is-positive@3.1.0')])
  })

  test('errors on version range patch failures', async () => {
    preparePackages([
      {
        name: 'foo',
        version: '0.0.0',
        private: true,
        dependencies: {
          'is-positive': '1.0.0', // is-positive@1.0.0.patch should succeed
        },
      },
      {
        name: 'bar',
        version: '0.0.0',
        private: true,
        dependencies: {
          'is-positive': '3.1.0', // is-positive@1.0.0.patch should fail
        },
      },
    ])

    const patchFile = f.find('patch-pkg/is-positive@1.0.0.patch')

    writeYamlFile('pnpm-workspace.yaml', {
      packages: ['**', '!store/**'],
      patchedDependencies: {
        'is-positive@>=1': patchFile,
      },
    })

    // pnpm install should fail
    const { status, stdout } = execPnpmSync(['install'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_PATCH_FAILED')
    const errorLines = stdout.toString().split('\n').filter(line => line.includes('ERR_PNPM_PATCH_FAILED'))
    expect(errorLines).toStrictEqual([expect.stringContaining(patchFile)])
    expect(errorLines).toStrictEqual([expect.stringContaining('is-positive@3.1.0')])
  })

  test('errors on star version range patch failures', async () => {
    preparePackages([
      {
        name: 'foo',
        version: '0.0.0',
        private: true,
        dependencies: {
          'is-positive': '1.0.0', // is-positive@1.0.0.patch should succeed
        },
      },
      {
        name: 'bar',
        version: '0.0.0',
        private: true,
        dependencies: {
          'is-positive': '3.1.0', // is-positive@1.0.0.patch should fail
        },
      },
    ])

    const patchFile = f.find('patch-pkg/is-positive@1.0.0.patch')

    writeYamlFile('pnpm-workspace.yaml', {
      packages: ['**', '!store/**'],
      patchedDependencies: {
        'is-positive@*': patchFile,
      },
    })

    // pnpm install should fail
    const { status, stdout } = execPnpmSync(['install'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_PATCH_FAILED')
    const errorLines = stdout.toString().split('\n').filter(line => line.includes('ERR_PNPM_PATCH_FAILED'))
    expect(errorLines).toStrictEqual([expect.stringContaining(patchFile)])
    expect(errorLines).toStrictEqual([expect.stringContaining('is-positive@3.1.0')])
  })

  test('allows name-only patches to fail with a warning (legacy behavior)', async () => {
    preparePackages([
      {
        name: 'foo',
        version: '0.0.0',
        private: true,
        dependencies: {
          'is-positive': '1.0.0', // is-positive@1.0.0.patch should succeed
        },
      },
      {
        name: 'bar',
        version: '0.0.0',
        private: true,
        dependencies: {
          'is-positive': '3.1.0', // is-positive@1.0.0.patch should fail
        },
      },
    ])

    const patchFile = f.find('patch-pkg/is-positive@1.0.0.patch')

    writeYamlFile('pnpm-workspace.yaml', {
      packages: ['**', '!store/**'],
      patchedDependencies: {
        'is-positive': patchFile,
      },
    })

    // pnpm install should not fail
    const { stdout } = execPnpmSync(['install'], { expectSuccess: true })

    // pnpm install should print a warning about patch application failure
    expect(stdout.toString()).toContain('Could not apply patch')

    // the patch should apply to is-positive@1.0.0
    expect(fs.readFileSync('foo/node_modules/is-positive/index.js', 'utf-8')).toContain('// patched')

    // the patch should not apply to is-positive@3.2.1
    expect(fs.readFileSync('bar/node_modules/is-positive/index.js', 'utf-8')).not.toContain('// patched')
  })
})

describe('ignorePatchFailures=true', () => {
  test('allows exact version patches to fail with a warning', async () => {
    preparePackages([
      {
        name: 'foo',
        version: '0.0.0',
        private: true,
        dependencies: {
          'is-positive': '1.0.0', // is-positive@1.0.0.patch should succeed
        },
      },
      {
        name: 'bar',
        version: '0.0.0',
        private: true,
        dependencies: {
          'is-positive': '3.1.0', // is-positive@1.0.0.patch should fail
        },
      },
    ])

    const patchFile = f.find('patch-pkg/is-positive@1.0.0.patch')

    writeYamlFile('pnpm-workspace.yaml', {
      packages: ['**', '!store/**'],
      patchedDependencies: {
        'is-positive@1.0.0': patchFile, // should succeed
        'is-positive@3.1.0': patchFile, // should fail
      },
      ignorePatchFailures: true,
    })

    // pnpm install should not fail
    const { stdout } = execPnpmSync(['install'], { expectSuccess: true })

    // pnpm install should print a warning about patch application failure
    expect(stdout.toString()).toContain('Could not apply patch')

    // the patch should apply to is-positive@1.0.0
    expect(fs.readFileSync('foo/node_modules/is-positive/index.js', 'utf-8')).toContain('// patched')

    // the patch should not apply to is-positive@3.2.1
    expect(fs.readFileSync('bar/node_modules/is-positive/index.js', 'utf-8')).not.toContain('// patched')
  })

  test('allows version range patches to fail with a warning', async () => {
    preparePackages([
      {
        name: 'foo',
        version: '0.0.0',
        private: true,
        dependencies: {
          'is-positive': '1.0.0', // is-positive@1.0.0.patch should succeed
        },
      },
      {
        name: 'bar',
        version: '0.0.0',
        private: true,
        dependencies: {
          'is-positive': '3.1.0', // is-positive@1.0.0.patch should fail
        },
      },
    ])

    const patchFile = f.find('patch-pkg/is-positive@1.0.0.patch')

    writeYamlFile('pnpm-workspace.yaml', {
      packages: ['**', '!store/**'],
      patchedDependencies: {
        'is-positive@>=1': patchFile,
      },
      ignorePatchFailures: true,
    })

    // pnpm install should not fail
    const { stdout } = execPnpmSync(['install'], { expectSuccess: true })

    // pnpm install should print a warning about patch application failure
    expect(stdout.toString()).toContain('Could not apply patch')

    // the patch should apply to is-positive@1.0.0
    expect(fs.readFileSync('foo/node_modules/is-positive/index.js', 'utf-8')).toContain('// patched')

    // the patch should not apply to is-positive@3.2.1
    expect(fs.readFileSync('bar/node_modules/is-positive/index.js', 'utf-8')).not.toContain('// patched')
  })

  test('allows star version range patches to fail with a warning', async () => {
    preparePackages([
      {
        name: 'foo',
        version: '0.0.0',
        private: true,
        dependencies: {
          'is-positive': '1.0.0', // is-positive@1.0.0.patch should succeed
        },
      },
      {
        name: 'bar',
        version: '0.0.0',
        private: true,
        dependencies: {
          'is-positive': '3.1.0', // is-positive@1.0.0.patch should fail
        },
      },
    ])

    const patchFile = f.find('patch-pkg/is-positive@1.0.0.patch')

    writeYamlFile('pnpm-workspace.yaml', {
      packages: ['**', '!store/**'],
      patchedDependencies: {
        'is-positive@*': patchFile,
      },
      ignorePatchFailures: true,
    })

    // pnpm install should not fail
    const { stdout } = execPnpmSync(['install'], { expectSuccess: true })

    // pnpm install should print a warning about patch application failure
    expect(stdout.toString()).toContain('Could not apply patch')

    // the patch should apply to is-positive@1.0.0
    expect(fs.readFileSync('foo/node_modules/is-positive/index.js', 'utf-8')).toContain('// patched')

    // the patch should not apply to is-positive@3.2.1
    expect(fs.readFileSync('bar/node_modules/is-positive/index.js', 'utf-8')).not.toContain('// patched')
  })

  test('allows name-only patches to fail with a warning', async () => {
    preparePackages([
      {
        name: 'foo',
        version: '0.0.0',
        private: true,
        dependencies: {
          'is-positive': '1.0.0', // is-positive@1.0.0.patch should succeed
        },
      },
      {
        name: 'bar',
        version: '0.0.0',
        private: true,
        dependencies: {
          'is-positive': '3.1.0', // is-positive@1.0.0.patch should fail
        },
      },
    ])

    const patchFile = f.find('patch-pkg/is-positive@1.0.0.patch')

    writeYamlFile('pnpm-workspace.yaml', {
      packages: ['**', '!store/**'],
      patchedDependencies: {
        'is-positive': patchFile,
      },
      ignorePatchFailures: true,
    })

    // pnpm install should not fail
    const { stdout } = execPnpmSync(['install'], { expectSuccess: true })

    // pnpm install should print a warning about patch application failure
    expect(stdout.toString()).toContain('Could not apply patch')

    // the patch should apply to is-positive@1.0.0
    expect(fs.readFileSync('foo/node_modules/is-positive/index.js', 'utf-8')).toContain('// patched')

    // the patch should not apply to is-positive@3.2.1
    expect(fs.readFileSync('bar/node_modules/is-positive/index.js', 'utf-8')).not.toContain('// patched')
  })
})

describe('ignorePatchFailures=false', () => {
  test('errors on exact version patch failures', async () => {
    preparePackages([
      {
        name: 'foo',
        version: '0.0.0',
        private: true,
        dependencies: {
          'is-positive': '1.0.0', // is-positive@1.0.0.patch should succeed
        },
      },
      {
        name: 'bar',
        version: '0.0.0',
        private: true,
        dependencies: {
          'is-positive': '3.1.0', // is-positive@1.0.0.patch should fail
        },
      },
    ])

    const patchFile = f.find('patch-pkg/is-positive@1.0.0.patch')

    writeYamlFile('pnpm-workspace.yaml', {
      packages: ['**', '!store/**'],
      patchedDependencies: {
        'is-positive@1.0.0': patchFile, // should succeed
        'is-positive@3.1.0': patchFile, // should fail
      },
      ignorePatchFailures: false,
    })

    // pnpm install should fail
    const { status, stdout } = execPnpmSync(['install'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_PATCH_FAILED')
    const errorLines = stdout.toString().split('\n').filter(line => line.includes('ERR_PNPM_PATCH_FAILED'))
    expect(errorLines).toStrictEqual([expect.stringContaining(patchFile)])
    expect(errorLines).toStrictEqual([expect.stringContaining('is-positive@3.1.0')])
  })

  test('errors on version range patch failures', async () => {
    preparePackages([
      {
        name: 'foo',
        version: '0.0.0',
        private: true,
        dependencies: {
          'is-positive': '1.0.0', // is-positive@1.0.0.patch should succeed
        },
      },
      {
        name: 'bar',
        version: '0.0.0',
        private: true,
        dependencies: {
          'is-positive': '3.1.0', // is-positive@1.0.0.patch should fail
        },
      },
    ])

    const patchFile = f.find('patch-pkg/is-positive@1.0.0.patch')

    writeYamlFile('pnpm-workspace.yaml', {
      packages: ['**', '!store/**'],
      patchedDependencies: {
        'is-positive@>=1': patchFile,
      },
      ignorePatchFailures: false,
    })

    // pnpm install not fail
    const { status, stdout } = execPnpmSync(['install'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_PATCH_FAILED')
    const errorLines = stdout.toString().split('\n').filter(line => line.includes('ERR_PNPM_PATCH_FAILED'))
    expect(errorLines).toStrictEqual([expect.stringContaining(patchFile)])
    expect(errorLines).toStrictEqual([expect.stringContaining('is-positive@3.1.0')])
  })

  test('errors on star version range patch failures', async () => {
    preparePackages([
      {
        name: 'foo',
        version: '0.0.0',
        private: true,
        dependencies: {
          'is-positive': '1.0.0', // is-positive@1.0.0.patch should succeed
        },
      },
      {
        name: 'bar',
        version: '0.0.0',
        private: true,
        dependencies: {
          'is-positive': '3.1.0', // is-positive@1.0.0.patch should fail
        },
      },
    ])

    const patchFile = f.find('patch-pkg/is-positive@1.0.0.patch')

    writeYamlFile('pnpm-workspace.yaml', {
      packages: ['**', '!store/**'],
      patchedDependencies: {
        'is-positive@*': patchFile,
      },
      ignorePatchFailures: false,
    })

    // pnpm install not fail
    const { status, stdout } = execPnpmSync(['install'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_PATCH_FAILED')
    const errorLines = stdout.toString().split('\n').filter(line => line.includes('ERR_PNPM_PATCH_FAILED'))
    expect(errorLines).toStrictEqual([expect.stringContaining(patchFile)])
    expect(errorLines).toStrictEqual([expect.stringContaining('is-positive@3.1.0')])
  })

  test('allows name-only patches to fail with a warning', async () => {
    preparePackages([
      {
        name: 'foo',
        version: '0.0.0',
        private: true,
        dependencies: {
          'is-positive': '1.0.0', // is-positive@1.0.0.patch should succeed
        },
      },
      {
        name: 'bar',
        version: '0.0.0',
        private: true,
        dependencies: {
          'is-positive': '3.1.0', // is-positive@1.0.0.patch should fail
        },
      },
    ])

    const patchFile = f.find('patch-pkg/is-positive@1.0.0.patch')

    writeYamlFile('pnpm-workspace.yaml', {
      packages: ['**', '!store/**'],
      patchedDependencies: {
        'is-positive': patchFile,
      },
      ignorePatchFailures: false,
    })

    // pnpm install should fail
    const { status, stdout } = execPnpmSync(['install'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_PATCH_FAILED')
    const errorLines = stdout.toString().split('\n').filter(line => line.includes('ERR_PNPM_PATCH_FAILED'))
    expect(errorLines).toStrictEqual([expect.stringContaining(patchFile)])
    expect(errorLines).toStrictEqual([expect.stringContaining('is-positive@3.1.0')])
  })
})
