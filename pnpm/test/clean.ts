import fs from 'fs'
import path from 'path'
import { preparePackages, tempDir } from '@pnpm/prepare'
import { sync as writeYamlFile } from 'write-yaml-file'
import { execPnpmSync } from './utils/index.js'

test('pnpm clean removes pnpm entries and packages but preserves non-pnpm hidden files', () => {
  tempDir()
  fs.writeFileSync('package.json', '{}', 'utf8')

  // Set up a fake node_modules with various entries
  fs.mkdirSync('node_modules/.pnpm', { recursive: true })
  fs.mkdirSync('node_modules/.bin')
  fs.writeFileSync('node_modules/.modules.yaml', 'storeDir: /tmp/store')
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
  writeYamlFile('pnpm-workspace.yaml', { packages: ['*'] })

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
  writeYamlFile('pnpm-workspace.yaml', { packages: ['*'] })

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
