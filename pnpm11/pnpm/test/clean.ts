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

  for (const modulesDir of ['.', 'escape/nm', 'escape/../keep-dir']) {
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

  for (const virtualStoreDir of ['.', 'escape/store']) {
    const result = execPnpmSync(['clean'], {
      env: { pnpm_config_virtual_store_dir: virtualStoreDir },
    })
    expect(result.status).toBe(0)
    expect(fs.existsSync('package.json')).toBe(true)
    expect(fs.existsSync(path.join(outsideStore, 'keep.txt'))).toBe(true)
  }
})
