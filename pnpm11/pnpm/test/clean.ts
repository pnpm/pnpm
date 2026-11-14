import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { preparePackages, tempDir } from '@pnpm/prepare'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpmSync } from './utils/index.js'

function writeJsonFile (filePath: string, obj: object): void {
  fs.mkdirSync(path.dirname(filePath), { recursive: true })
  fs.writeFileSync(filePath, JSON.stringify(obj))
}

test('pnpm clean removes pnpm entries and packages but preserves non-pnpm hidden files', () => {
  tempDir()
  fs.writeFileSync('package.json', '{}', 'utf8')

  // Set up a fake node_modules with various entries
  fs.mkdirSync('node_modules/.pnpm', { recursive: true })
  fs.mkdirSync('node_modules/.bin')
  fs.writeFileSync('node_modules/.modules.yaml', 'storeDir: /tmp/store')
  fs.writeFileSync('node_modules/.pnpm-workspace-state-v1.json', '{}')
  fs.mkdirSync('node_modules/.cache')
  fs.writeFileSync('node_modules/.cache/some-file', 'cached')
  fs.mkdirSync('node_modules/lodash')
  fs.writeFileSync('node_modules/lodash/index.js', '')
  fs.mkdirSync('node_modules/@scope/pkg', { recursive: true })
  fs.writeFileSync('node_modules/@scope/pkg/index.js', '')

  const result = execPnpmSync(['clean'])
  expect(result.status).toBe(0)

  // Pnpm entries should be removed
  expect(fs.existsSync('node_modules/.pnpm')).toBe(false)
  expect(fs.existsSync('node_modules/.bin')).toBe(false)
  expect(fs.existsSync('node_modules/.modules.yaml')).toBe(false)
  expect(fs.existsSync('node_modules/.pnpm-workspace-state-v1.json')).toBe(false)

  // Regular packages should be removed
  expect(fs.existsSync('node_modules/lodash')).toBe(false)
  expect(fs.existsSync('node_modules/@scope')).toBe(false)

  // Non-pnpm hidden files should be preserved
  expect(fs.existsSync('node_modules/.cache')).toBe(true)
  expect(fs.existsSync('node_modules/.cache/some-file')).toBe(true)
})

test('pnpm clean handles missing node_modules gracefully', () => {
  tempDir()
  fs.writeFileSync('package.json', '{}', 'utf8')

  // node_modules does not exist
  expect(fs.existsSync('node_modules')).toBe(false)

  const result = execPnpmSync(['clean'])
  expect(result.status).toBe(0)
})

test('pnpm clean preserves lockfile by default', () => {
  tempDir()
  fs.writeFileSync('package.json', '{}', 'utf8')
  fs.writeFileSync('pnpm-lock.yaml', 'lockfileVersion: 9')
  fs.mkdirSync('node_modules/.pnpm', { recursive: true })

  const result = execPnpmSync(['clean'])
  expect(result.status).toBe(0)

  expect(fs.existsSync('node_modules/.pnpm')).toBe(false)
  expect(fs.existsSync('pnpm-lock.yaml')).toBe(true)
})

test('pnpm clean preserves lockfile when pnpm-workspace.yaml sets lockfile', () => {
  tempDir()
  fs.writeFileSync('package.json', '{}', 'utf8')
  writeYamlFileSync('pnpm-workspace.yaml', { lockfile: true })
  fs.writeFileSync('pnpm-lock.yaml', 'lockfileVersion: 9')
  fs.mkdirSync('node_modules/.pnpm', { recursive: true })

  const result = execPnpmSync(['clean'])
  expect(result.status).toBe(0)

  expect(fs.existsSync('node_modules/.pnpm')).toBe(false)
  expect(fs.existsSync('pnpm-lock.yaml')).toBe(true)
})

test('pnpm clean --lockfile removes lockfile', () => {
  tempDir()
  fs.writeFileSync('package.json', '{}', 'utf8')
  fs.writeFileSync('pnpm-lock.yaml', 'lockfileVersion: 9')
  fs.mkdirSync('node_modules/.pnpm', { recursive: true })

  const result = execPnpmSync(['clean', '--lockfile'])
  expect(result.status).toBe(0)

  expect(fs.existsSync('node_modules/.pnpm')).toBe(false)
  expect(fs.existsSync('pnpm-lock.yaml')).toBe(false)
})

test('pnpm clean works in a workspace', () => {
  preparePackages([
    { name: 'project-a', version: '1.0.0' },
    { name: 'project-b', version: '1.0.0' },
  ])

  fs.writeFileSync('package.json', JSON.stringify({ name: 'root', version: '1.0.0' }))
  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['*'] })

  // Set up fake node_modules in root and each project
  for (const dir of ['.', 'project-a', 'project-b']) {
    fs.mkdirSync(path.join(dir, 'node_modules', '.pnpm'), { recursive: true })
    fs.mkdirSync(path.join(dir, 'node_modules', '.bin'))
    fs.writeFileSync(path.join(dir, 'node_modules', '.modules.yaml'), 'storeDir: /tmp/store')
    fs.mkdirSync(path.join(dir, 'node_modules', 'some-pkg'))
    fs.writeFileSync(path.join(dir, 'node_modules', 'some-pkg', 'index.js'), '')
    // Non-pnpm hidden file
    fs.mkdirSync(path.join(dir, 'node_modules', '.cache'))
    fs.writeFileSync(path.join(dir, 'node_modules', '.cache', 'data'), 'cached')
  }

  const result = execPnpmSync(['clean'])
  expect(result.status).toBe(0)

  for (const dir of ['.', 'project-a', 'project-b']) {
    expect(fs.existsSync(path.join(dir, 'node_modules', '.pnpm'))).toBe(false)
    expect(fs.existsSync(path.join(dir, 'node_modules', '.bin'))).toBe(false)
    expect(fs.existsSync(path.join(dir, 'node_modules', '.modules.yaml'))).toBe(false)
    expect(fs.existsSync(path.join(dir, 'node_modules', 'some-pkg'))).toBe(false)
    // Non-pnpm hidden files preserved
    expect(fs.existsSync(path.join(dir, 'node_modules', '.cache', 'data'))).toBe(true)
  }
})

test('pnpm clean removes custom virtual-store-dir inside the project', () => {
  tempDir()
  fs.writeFileSync('package.json', '{}', 'utf8')

  // Custom virtual store outside node_modules but inside the project
  fs.mkdirSync('.pnpm-store/v3', { recursive: true })
  fs.writeFileSync('.pnpm-store/v3/some-pkg.tgz', '')
  fs.mkdirSync('node_modules/lodash', { recursive: true })

  const result = execPnpmSync(['clean'], {
    env: { pnpm_config_virtual_store_dir: '.pnpm-store' },
  })
  expect(result.status).toBe(0)

  expect(fs.existsSync('.pnpm-store')).toBe(false)
  expect(fs.existsSync('node_modules/lodash')).toBe(false)
})

test('pnpm clean does not remove virtual-store-dir outside the project root', () => {
  tempDir()
  fs.writeFileSync('package.json', '{}', 'utf8')

  // Virtual store outside the project root
  const outsideDir = path.resolve('..', 'outside-store')
  fs.mkdirSync(outsideDir, { recursive: true })
  fs.writeFileSync(path.join(outsideDir, 'data'), 'keep')

  const result = execPnpmSync(['clean'], {
    env: { pnpm_config_virtual_store_dir: outsideDir },
  })
  expect(result.status).toBe(0)

  // Should NOT be removed since it's outside the project root
  expect(fs.existsSync(path.join(outsideDir, 'data'))).toBe(true)
})

test('pnpm clean --lockfile removes lockfiles in workspace', () => {
  preparePackages([
    { name: 'project-a', version: '1.0.0' },
    { name: 'project-b', version: '1.0.0' },
  ])

  fs.writeFileSync('package.json', JSON.stringify({ name: 'root', version: '1.0.0' }))
  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['*'] })

  // Set up lockfiles and node_modules
  for (const dir of ['.', 'project-a', 'project-b']) {
    fs.writeFileSync(path.join(dir, 'pnpm-lock.yaml'), 'lockfileVersion: 9')
    fs.mkdirSync(path.join(dir, 'node_modules', '.pnpm'), { recursive: true })
  }

  const result = execPnpmSync(['clean', '--lockfile'])
  expect(result.status).toBe(0)

  for (const dir of ['.', 'project-a', 'project-b']) {
    expect(fs.existsSync(path.join(dir, 'node_modules', '.pnpm'))).toBe(false)
    expect(fs.existsSync(path.join(dir, 'pnpm-lock.yaml'))).toBe(false)
  }
})

test('pnpm clean runs the clean script from package.json when present', () => {
  tempDir()
  writeJsonFile('package.json', {
    name: 'has-clean-script',
    scripts: { clean: 'echo "script-clean-ran"' },
  })

  const result = execPnpmSync(['clean'])
  expect(result.status).toBe(0)
  expect(result.stdout.toString()).toContain('script-clean-ran')
})

test('pnpm purge runs the built-in clean even when a clean script exists', () => {
  tempDir()
  writeJsonFile('package.json', {
    name: 'has-clean-script',
    scripts: { clean: 'echo "script-clean-ran"' },
  })
  fs.mkdirSync('node_modules/.pnpm', { recursive: true })

  const result = execPnpmSync(['purge'])
  expect(result.status).toBe(0)
  expect(result.stdout.toString()).not.toContain('script-clean-ran')
  expect(fs.existsSync('node_modules/.pnpm')).toBe(false)
})

test('pnpm purge runs the purge script from package.json when present', () => {
  tempDir()
  writeJsonFile('package.json', {
    name: 'has-purge-script',
    scripts: { purge: 'echo "script-purge-ran"' },
  })

  const result = execPnpmSync(['purge'])
  expect(result.status).toBe(0)
  expect(result.stdout.toString()).toContain('script-purge-ran')
})

test('pnpm clean runs the built-in when no clean script exists', () => {
  tempDir()
  writeJsonFile('package.json', { name: 'no-clean-script' })
  fs.mkdirSync('node_modules/.pnpm', { recursive: true })

  const result = execPnpmSync(['clean'])
  expect(result.status).toBe(0)
  expect(fs.existsSync('node_modules/.pnpm')).toBe(false)
})

test('pnpm clean errors in workspace subdir when root has clean script', () => {
  preparePackages([
    { name: 'project-a', version: '1.0.0' },
  ])

  writeJsonFile('package.json', {
    name: 'root',
    version: '1.0.0',
    scripts: { clean: 'echo "root-clean"' },
  })
  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['*'] })

  const result = execPnpmSync(['clean'], { cwd: path.resolve('project-a') })
  expect(result.status).toBe(1)
  const output = result.stdout.toString() + result.stderr.toString()
  expect(output).toContain('ERR_PNPM_SCRIPT_OVERRIDE_IN_WORKSPACE_ROOT')
})

test('pnpm pm clean runs the built-in command even when a clean script exists', () => {
  tempDir()
  writeJsonFile('package.json', {
    name: 'has-clean-script',
    scripts: { clean: 'echo "script-clean-ran"' },
  })
  fs.mkdirSync('node_modules/.pnpm', { recursive: true })

  const result = execPnpmSync(['pm', 'clean'])
  expect(result.status).toBe(0)
  expect(result.stdout.toString()).not.toContain('script-clean-ran')
  expect(fs.existsSync('node_modules/.pnpm')).toBe(false)
})

test('pnpm pm clean does not error in workspace subdir when root has clean script', () => {
  preparePackages([
    { name: 'project-a', version: '1.0.0' },
  ])

  writeJsonFile('package.json', {
    name: 'root',
    version: '1.0.0',
    scripts: { clean: 'echo "root-clean"' },
  })
  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['*'] })
  fs.mkdirSync(path.join('project-a', 'node_modules', '.pnpm'), { recursive: true })

  const result = execPnpmSync(['pm', 'clean'], { cwd: path.resolve('project-a') })
  expect(result.status).toBe(0)
  expect(fs.existsSync(path.join('project-a', 'node_modules', '.pnpm'))).toBe(false)
})

test('pnpm clean runs built-in in workspace subdir when root has no clean script', () => {
  preparePackages([
    { name: 'project-a', version: '1.0.0' },
  ])

  writeJsonFile('package.json', { name: 'root', version: '1.0.0' })
  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['*'] })
  fs.mkdirSync(path.join('project-a', 'node_modules', '.pnpm'), { recursive: true })

  const result = execPnpmSync(['clean'], { cwd: path.resolve('project-a') })
  expect(result.status).toBe(0)
})

test.each(['clean', 'purge'])('pnpm %s does not delete files in external modulesDir', (command) => {
  for (const useAbsolute of [false, true]) {
    tempDir()
    const externalTarget = path.resolve('..', 'external-target')
    fs.mkdirSync(path.join(externalTarget, 'lodash'), { recursive: true })
    fs.writeFileSync(path.join(externalTarget, 'keep.txt'), 'keep')
    fs.writeFileSync(path.join(externalTarget, 'lodash', 'package.json'), '{}')

    writeJsonFile('package.json', { name: 'demo-app', version: '1.0.0' })
    writeYamlFileSync('pnpm-workspace.yaml', {
      modulesDir: useAbsolute ? externalTarget : '../external-target',
    })

    const result = execPnpmSync([command])
    expect(result.status).toBe(0)
    expect(fs.existsSync(path.join(externalTarget, 'keep.txt'))).toBe(true)
    expect(fs.existsSync(path.join(externalTarget, 'lodash', 'package.json'))).toBe(true)
  }
})

test('pnpm clean rejects self, symlinked parent, and sibling modulesDir escapes', () => {
  tempDir()
  const outsideSub = path.resolve('..', 'outside', 'sub')
  fs.mkdirSync(path.join(outsideSub, 'nm'), { recursive: true })
  fs.writeFileSync(path.join(outsideSub, 'nm', 'keep.txt'), 'keep')
  fs.symlinkSync(outsideSub, 'escape', process.platform === 'win32' ? 'junction' : 'dir')

  fs.mkdirSync('keep-dir', { recursive: true })
  fs.writeFileSync(path.join('keep-dir', 'keep.txt'), 'keep')
  writeJsonFile('package.json', { name: 'demo-app', version: '1.0.0' })

  const drivePrefix = path.parse(process.cwd()).root.slice(0, 2)
  const escapeCases = [
    '.',
    '. ',
    '.. ',
    'escape/nm',
    'escape/../keep-dir',
    ...(process.platform === 'win32'
      ? ['escape\\nm', 'escape\\..\\keep-dir', `${drivePrefix}escape\\..\\keep-dir`]
      : []),
  ]
  for (const modulesDir of escapeCases) {
    writeYamlFileSync('pnpm-workspace.yaml', { modulesDir })
    const result = execPnpmSync(['clean'])
    expect(result.status).toBe(0)
    expect(fs.existsSync('package.json')).toBe(true)
    expect(fs.existsSync(path.join(outsideSub, 'nm', 'keep.txt'))).toBe(true)
    expect(fs.existsSync(path.join('keep-dir', 'keep.txt'))).toBe(true)
  }

  preparePackages([
    { location: 'packages/project-a', package: { name: 'project-a', version: '1.0.0' } },
    { location: 'packages/project-b', package: { name: 'project-b', version: '1.0.0' } },
  ])
  writeJsonFile('package.json', { name: 'root', version: '1.0.0' })
  fs.writeFileSync(path.join('packages', 'project-b', 'keep.txt'), 'keep')
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['packages/*'],
    modulesDir: '../project-b',
  })

  const wsResult = execPnpmSync(['clean'])
  expect(wsResult.status).toBe(0)
  expect(fs.existsSync(path.join('packages', 'project-b', 'package.json'))).toBe(true)
  expect(fs.existsSync(path.join('packages', 'project-b', 'keep.txt'))).toBe(true)
})

test.each(['clean', 'purge'])('pnpm %s unlinks symlinked node_modules without deleting target contents', (command) => {
  tempDir()
  const externalTarget = path.resolve('..', 'external-target')
  fs.mkdirSync(path.join(externalTarget, '.pnpm'), { recursive: true })
  fs.mkdirSync(path.join(externalTarget, 'lodash'), { recursive: true })
  fs.writeFileSync(path.join(externalTarget, 'keep.txt'), 'keep')
  fs.writeFileSync(path.join(externalTarget, 'lodash', 'package.json'), '{}')

  writeJsonFile('package.json', { name: 'demo-app', version: '1.0.0' })
  fs.symlinkSync(externalTarget, 'node_modules', process.platform === 'win32' ? 'junction' : 'dir')

  const result = execPnpmSync([command])
  expect(result.status).toBe(0)
  expect(result.stdout.toString()).toContain('Removing node_modules')
  expect(() => fs.lstatSync('node_modules')).toThrow()
  expect(fs.existsSync(path.join(externalTarget, 'keep.txt'))).toBe(true)
  expect(fs.existsSync(path.join(externalTarget, '.pnpm'))).toBe(true)
  expect(fs.existsSync(path.join(externalTarget, 'lodash', 'package.json'))).toBe(true)
})

test('pnpm clean keeps project root and rejects symlinked parent for virtual-store-dir', () => {
  tempDir()
  const outsideStore = path.resolve('..', 'outside', 'store')
  fs.mkdirSync(outsideStore, { recursive: true })
  fs.writeFileSync(path.join(outsideStore, 'keep.txt'), 'keep')
  fs.symlinkSync(path.dirname(outsideStore), 'escape', process.platform === 'win32' ? 'junction' : 'dir')
  writeJsonFile('package.json', { name: 'demo-app', version: '1.0.0' })

  fs.mkdirSync('pnpm-lock.yaml', { recursive: true })
  fs.writeFileSync(path.join('pnpm-lock.yaml', 'keep.txt'), 'keep')

  for (const virtualStoreDir of ['.', '. ', '.. ', 'escape/store']) {
    writeYamlFileSync('pnpm-workspace.yaml', { virtualStoreDir })
    const result = execPnpmSync(['clean', '--lockfile'])
    expect(result.status).toBe(0)
    expect(fs.existsSync('package.json')).toBe(true)
    expect(fs.existsSync(path.join(outsideStore, 'keep.txt'))).toBe(true)
    expect(fs.existsSync(path.join('pnpm-lock.yaml', 'keep.txt'))).toBe(true)
  }
})

test('pnpm clean honors --filter, --filter-prod, and -w', () => {
  preparePackages([
    {
      location: 'packages/pkg-a',
      package: { name: 'pkg-a', version: '1.0.0' },
    },
    {
      location: 'packages/pkg-b',
      package: {
        name: 'pkg-b',
        version: '1.0.0',
        dependencies: { 'pkg-a': 'workspace:*' },
      },
    },
  ])
  writeJsonFile('package.json', { name: 'root-pkg', version: '1.0.0', private: true })
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['packages/*'],
    virtualStoreDir: '.virtual-store',
  })

  const seedAll = (): void => {
    fs.mkdirSync(path.join('.virtual-store', 'entry'), { recursive: true })
    for (const dir of ['.', 'packages/pkg-a', 'packages/pkg-b']) {
      fs.mkdirSync(path.join(dir, 'node_modules', 'seeded'), { recursive: true })
    }
  }

  seedAll()
  expect(execPnpmSync(['--filter', 'pkg-a', 'clean']).status).toBe(0)
  expect(fs.existsSync(path.join('packages/pkg-a', 'node_modules', 'seeded'))).toBe(false)
  expect(fs.existsSync(path.join('packages/pkg-b', 'node_modules', 'seeded'))).toBe(true)
  expect(fs.existsSync(path.join('node_modules', 'seeded'))).toBe(true)
  expect(fs.existsSync(path.join('.virtual-store', 'entry'))).toBe(true)

  seedAll()
  expect(execPnpmSync(['--filter-prod', '...pkg-a', 'clean']).status).toBe(0)
  expect(fs.existsSync(path.join('packages/pkg-a', 'node_modules', 'seeded'))).toBe(false)
  expect(fs.existsSync(path.join('packages/pkg-b', 'node_modules', 'seeded'))).toBe(false)
  expect(fs.existsSync(path.join('node_modules', 'seeded'))).toBe(true)

  seedAll()
  expect(execPnpmSync(['-w', 'clean']).status).toBe(0)
  expect(fs.existsSync(path.join('node_modules', 'seeded'))).toBe(false)
  expect(fs.existsSync(path.join('packages/pkg-a', 'node_modules', 'seeded'))).toBe(true)

  // Non-CLI filter via environment variable must also narrow `pnpm clean`
  seedAll()
  expect(execPnpmSync(['clean'], { env: { pnpm_config_filter: 'pkg-a' } }).status).toBe(0)
  expect(fs.existsSync(path.join('packages/pkg-a', 'node_modules', 'seeded'))).toBe(false)
  expect(fs.existsSync(path.join('packages/pkg-b', 'node_modules', 'seeded'))).toBe(true)
  expect(fs.existsSync(path.join('node_modules', 'seeded'))).toBe(true)
})

test('pnpm clean cleans root node_modules in workspace without root package.json', () => {
  preparePackages([
    {
      location: 'packages/pkg-a',
      package: { name: 'pkg-a', version: '1.0.0' },
    },
  ])
  if (fs.existsSync('package.json')) {
    fs.unlinkSync('package.json')
  }
  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['packages/*'] })
  fs.mkdirSync(path.join('node_modules', '.pnpm'), { recursive: true })
  fs.mkdirSync(path.join('node_modules', '.cache'), { recursive: true })
  fs.writeFileSync(path.join('node_modules', '.cache', 'keep'), 'keep')
  fs.mkdirSync(path.join('packages', 'pkg-a', 'node_modules', 'pkg'), { recursive: true })

  expect(execPnpmSync(['clean']).status).toBe(0)
  expect(fs.existsSync(path.join('node_modules', '.pnpm'))).toBe(false)
  expect(fs.existsSync(path.join('node_modules', '.cache', 'keep'))).toBe(true)
  expect(fs.existsSync(path.join('packages', 'pkg-a', 'node_modules', 'pkg'))).toBe(false)
})

test('pnpm clean --lockfile respects lockfileDir containment and per-project lockfiles', () => {
  preparePackages([
    { location: 'packages/pkg-a', package: { name: 'pkg-a', version: '1.0.0' } },
    { location: 'packages/pkg-b', package: { name: 'pkg-b', version: '1.0.0' } },
  ])
  if (fs.existsSync('package.json')) {
    fs.unlinkSync('package.json')
  }

  // 1. Inside-root lockfileDir via pnpm-workspace.yaml and --lockfile-dir
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['packages/*'],
    lockfileDir: 'config/lock',
  })
  fs.mkdirSync(path.join('config', 'lock', 'node_modules', '.pnpm'), { recursive: true })
  fs.writeFileSync(path.join('config', 'lock', 'pnpm-lock.yaml'), 'lockfileVersion: 9')

  expect(execPnpmSync(['clean', '--lockfile']).status).toBe(0)
  expect(fs.existsSync(path.join('config', 'lock', 'pnpm-lock.yaml'))).toBe(false)
  expect(fs.existsSync(path.join('config', 'lock', 'node_modules', '.pnpm'))).toBe(false)

  fs.mkdirSync('cli-lock', { recursive: true })
  fs.writeFileSync(path.join('cli-lock', 'pnpm-lock.yaml'), 'lockfileVersion: 9')
  expect(execPnpmSync(['clean', '--lockfile', '--lockfile-dir', 'cli-lock']).status).toBe(0)
  expect(fs.existsSync(path.join('cli-lock', 'pnpm-lock.yaml'))).toBe(false)

  // 2. Containment: external lockfileDir and symlinked-parent escape must be left untouched
  const outsideLock = path.resolve('..', 'outside-lock')
  fs.mkdirSync(path.join(outsideLock, 'sub'), { recursive: true })
  fs.writeFileSync(path.join(outsideLock, 'pnpm-lock.yaml'), 'lockfileVersion: 9')
  fs.writeFileSync(path.join(outsideLock, 'sub', 'pnpm-lock.yaml'), 'lockfileVersion: 9')
  fs.symlinkSync(path.join(outsideLock, 'sub'), 'escape', process.platform === 'win32' ? 'junction' : 'dir')
  fs.mkdirSync('keep-dir', { recursive: true })
  fs.writeFileSync(path.join('keep-dir', 'pnpm-lock.yaml'), 'lockfileVersion: 9')

  for (const escaped of ['../outside-lock', 'escape', 'escape/../keep-dir']) {
    writeYamlFileSync('pnpm-workspace.yaml', {
      packages: ['packages/*'],
      lockfileDir: escaped,
    })
    expect(execPnpmSync(['clean', '--lockfile']).status).toBe(0)
    expect(fs.existsSync(path.join(outsideLock, 'pnpm-lock.yaml'))).toBe(true)
    expect(fs.existsSync(path.join(outsideLock, 'sub', 'pnpm-lock.yaml'))).toBe(true)
    expect(fs.existsSync(path.join('keep-dir', 'pnpm-lock.yaml'))).toBe(true)
  }

  // `--config.lockfile-dir` and `${ENV}` interpolation in pnpm-workspace.yaml also preserve containment
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['packages/*'],
    lockfileDir: '${ESCAPE_LOCKFILE_DIR}',
  })
  expect(execPnpmSync(['clean', '--lockfile'], { env: { ESCAPE_LOCKFILE_DIR: 'escape/../keep-dir' } }).status).toBe(0)
  expect(fs.existsSync(path.join('keep-dir', 'pnpm-lock.yaml'))).toBe(true)

  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['packages/*'] })
  expect(execPnpmSync(['--config.lockfile-dir=escape/../keep-dir', 'clean', '--lockfile']).status).toBe(0)
  expect(fs.existsSync(path.join('keep-dir', 'pnpm-lock.yaml'))).toBe(true)

  // `PNPM_CONFIG_LOCKFILE_DIR` preserves symlink-parent containment and resolves relative to cwd
  expect(execPnpmSync(['clean', '--lockfile'], { env: { PNPM_CONFIG_LOCKFILE_DIR: 'escape/../keep-dir' } }).status).toBe(0)
  expect(fs.existsSync(path.join('keep-dir', 'pnpm-lock.yaml'))).toBe(true)

  fs.mkdirSync('yaml-lock', { recursive: true })
  fs.writeFileSync(path.join('yaml-lock', 'pnpm-lock.yaml'), 'lockfileVersion: 9')
  fs.mkdirSync(path.join('packages', 'sub-lock'), { recursive: true })
  fs.writeFileSync(path.join('packages', 'sub-lock', 'pnpm-lock.yaml'), 'lockfileVersion: 9')
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['packages/*'],
    lockfileDir: 'yaml-lock',
  })
  expect(execPnpmSync(['clean', '--lockfile'], {
    cwd: path.resolve('packages/pkg-a'),
    env: { PNPM_CONFIG_LOCKFILE_DIR: '../sub-lock' },
  }).status).toBe(0)
  expect(fs.existsSync(path.join('packages', 'sub-lock', 'pnpm-lock.yaml'))).toBe(false)
  expect(fs.existsSync(path.join('yaml-lock', 'pnpm-lock.yaml'))).toBe(true)

  // 3. Per-project lockfiles with sharedWorkspaceLockfile: false (filtered and unfiltered)
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['packages/*'],
    sharedWorkspaceLockfile: false,
  })
  fs.writeFileSync(path.join('packages/pkg-a', 'pnpm-lock.yaml'), 'lockfileVersion: 9')
  fs.writeFileSync(path.join('packages/pkg-b', 'pnpm-lock.yaml'), 'lockfileVersion: 9')

  expect(execPnpmSync(['--filter', 'pkg-a', 'clean', '--lockfile']).status).toBe(0)
  expect(fs.existsSync(path.join('packages/pkg-a', 'pnpm-lock.yaml'))).toBe(false)
  expect(fs.existsSync(path.join('packages/pkg-b', 'pnpm-lock.yaml'))).toBe(true)

  expect(execPnpmSync(['clean', '--lockfile']).status).toBe(0)
  expect(fs.existsSync(path.join('packages/pkg-b', 'pnpm-lock.yaml'))).toBe(false)
})

test('pnpm clean preserves shared .pnpm on -w and --no-recursive and honors packageConfigs modulesDir', () => {
  preparePackages([
    {
      location: 'packages/pkg-a',
      package: {
        name: 'pkg-a',
        version: '1.0.0',
        dependencies: { 'is-negative': '1.0.0' },
      },
    },
  ])
  writeJsonFile('package.json', {
    name: 'root-pkg',
    version: '1.0.0',
    private: true,
    dependencies: { 'is-positive': '1.0.0' },
  })
  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['packages/*'] })

  expect(execPnpmSync(['install']).status).toBe(0)
  expect(execPnpmSync(['-w', 'clean']).status).toBe(0)
  expect(fs.existsSync(path.join('node_modules', 'is-positive'))).toBe(false)
  expect(fs.existsSync(path.join('node_modules', '.pnpm'))).toBe(true)
  expect(fs.existsSync(path.join('packages', 'pkg-a', 'node_modules', 'is-negative', 'package.json'))).toBe(true)

  expect(execPnpmSync(['install']).status).toBe(0)
  expect(execPnpmSync(['clean', '--no-recursive'], { cwd: path.resolve('packages/pkg-a') }).status).toBe(0)
  expect(fs.existsSync(path.join('packages', 'pkg-a', 'node_modules', 'is-negative'))).toBe(false)
  expect(fs.existsSync(path.join('node_modules', 'is-positive', 'package.json'))).toBe(true)
  expect(fs.existsSync(path.join('node_modules', '.pnpm'))).toBe(true)

  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['packages/*'],
    sharedWorkspaceLockfile: false,
    packageConfigs: {
      'pkg-a': {
        modulesDir: 'custom_nm',
      },
    },
  })
  fs.mkdirSync(path.join('packages', 'pkg-a', 'custom_nm', 'seeded'), { recursive: true })
  expect(execPnpmSync(['clean']).status).toBe(0)
  expect(fs.existsSync(path.join('packages', 'pkg-a', 'custom_nm', 'seeded'))).toBe(false)
})

test('pnpm clean protects rootless workspace member dirs when modulesDir overlaps and preserves shared .pnpm on --lockfile-dir .', () => {
  preparePackages([
    {
      location: 'packages/pkg-a',
      package: {
        name: 'pkg-a',
        version: '1.0.0',
      },
    },
  ])
  if (fs.existsSync('package.json')) {
    fs.unlinkSync('package.json')
  }
  fs.mkdirSync(path.join('packages', 'pkg-a', 'src'), { recursive: true })
  fs.writeFileSync(path.join('packages', 'pkg-a', 'src', 'index.js'), 'module.exports = 1\n')
  fs.mkdirSync(path.join('packages', 'pkg-a', 'packages', 'seeded-dep'), { recursive: true })
  fs.writeFileSync(path.join('packages', 'pkg-a', 'packages', 'seeded-dep', 'package.json'), '{"name":"seeded-dep"}')
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['packages/*'],
    modulesDir: 'packages',
  })

  expect(execPnpmSync(['clean']).status).toBe(0)
  expect(fs.existsSync(path.join('packages', 'pkg-a', 'package.json'))).toBe(true)
  expect(fs.existsSync(path.join('packages', 'pkg-a', 'src', 'index.js'))).toBe(true)
  expect(fs.existsSync(path.join('packages', 'pkg-a', 'packages', 'seeded-dep'))).toBe(false)

  // Shared workspace with explicit `--lockfile-dir .` preserves `.pnpm` and `.modules.yaml` on filtered root clean
  writeJsonFile('package.json', {
    name: 'root-pkg',
    version: '1.0.0',
    private: true,
    dependencies: { 'is-positive': '1.0.0' },
  })
  writeJsonFile(path.join('packages', 'pkg-a', 'package.json'), {
    name: 'pkg-a',
    version: '1.0.0',
    dependencies: { 'is-negative': '1.0.0' },
  })
  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['packages/*'] })
  expect(execPnpmSync(['install']).status).toBe(0)
  expect(execPnpmSync(['--filter', 'root-pkg', '--lockfile-dir', '.', 'clean']).status).toBe(0)
  expect(fs.existsSync(path.join('node_modules', 'is-positive'))).toBe(false)
  expect(fs.existsSync(path.join('node_modules', '.pnpm'))).toBe(true)
  expect(fs.existsSync(path.join('node_modules', '.modules.yaml'))).toBe(true)
  expect(fs.existsSync(path.join('packages', 'pkg-a', 'node_modules', 'is-negative', 'package.json'))).toBe(true)
})

test('pnpm clean skips installed packages under root custom_nm in rootless ** workspace and protects members when root manifest exists', () => {
  preparePackages([
    {
      location: 'packages/pkg-a',
      package: {
        name: 'pkg-a',
        version: '1.0.0',
      },
    },
  ])
  if (fs.existsSync('package.json')) {
    fs.unlinkSync('package.json')
  }
  fs.mkdirSync(path.join('custom_nm', 'lodash'), { recursive: true })
  fs.writeFileSync(path.join('custom_nm', 'lodash', 'package.json'), '{"name":"lodash"}')
  fs.mkdirSync(path.join('custom_nm', 'broken'), { recursive: true })
  fs.writeFileSync(path.join('custom_nm', 'broken', 'package.json'), '{ invalid json')
  fs.mkdirSync(path.join('packages', 'pkg-a', 'custom_nm', 'member-dep'), { recursive: true })
  fs.writeFileSync(path.join('packages', 'pkg-a', 'custom_nm', 'member-dep', 'package.json'), '{"name":"member-dep"}')
  fs.mkdirSync(path.join('packages', 'pkg-a', 'custom_nm', 'broken'), { recursive: true })
  fs.writeFileSync(path.join('packages', 'pkg-a', 'custom_nm', 'broken', 'package.json'), '{ invalid json')
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['**'],
    modulesDir: 'custom_nm',
  })

  expect(execPnpmSync(['clean']).status).toBe(0)
  expect(fs.existsSync(path.join('custom_nm', 'lodash'))).toBe(false)
  expect(fs.existsSync(path.join('custom_nm', 'broken'))).toBe(false)
  expect(fs.existsSync(path.join('packages', 'pkg-a', 'custom_nm', 'member-dep'))).toBe(false)
  expect(fs.existsSync(path.join('packages', 'pkg-a', 'package.json'))).toBe(true)

  // Workspace WITH root package.json and modulesDir overlapping packages/* preserves workspace members
  writeJsonFile('package.json', { name: 'root-pkg', version: '1.0.0', private: true })
  fs.mkdirSync(path.join('packages', 'pkg-a', 'src'), { recursive: true })
  fs.writeFileSync(path.join('packages', 'pkg-a', 'src', 'index.js'), 'module.exports = 1\n')
  fs.mkdirSync(path.join('packages', 'pkg-a', 'packages', 'seeded-dep'), { recursive: true })
  fs.writeFileSync(path.join('packages', 'pkg-a', 'packages', 'seeded-dep', 'package.json'), '{"name":"seeded-dep"}')
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['packages/*'],
    modulesDir: 'packages',
  })

  expect(execPnpmSync(['clean']).status).toBe(0)
  expect(fs.existsSync(path.join('packages', 'pkg-a', 'package.json'))).toBe(true)
  expect(fs.existsSync(path.join('packages', 'pkg-a', 'src', 'index.js'))).toBe(true)
  expect(fs.existsSync(path.join('packages', 'pkg-a', 'packages', 'seeded-dep'))).toBe(false)
})
