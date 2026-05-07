import path from 'node:path'
import fs from 'node:fs'

import { expect, test, describe, beforeEach } from '@jest/globals'
import { tempDir } from '@pnpm/prepare'
import { handler, cliOptionsTypes } from '../lib/pkg.js'

describe('pkg command', () => {
  let tmpDir: string

  beforeEach(() => {
    tmpDir = tempDir()
  })

  describe('get subcommand', () => {
    test('gets all fields when no keys provided', async () => {
      const manifest = {
        name: 'test-package',
        version: '1.0.0',
        dependencies: { foo: '1.0.0' },
      }
      fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

      const result = await handler({ dir: tmpDir }, ['get'])
      const parsed = JSON.parse(result as string)
      expect(parsed.name).toBe('test-package')
      expect(parsed.version).toBe('1.0.0')
    })

    test('gets specific keys', async () => {
      const manifest = {
        name: 'test-package',
        version: '1.0.0',
        description: 'A test package',
      }
      fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

      const result = await handler({ dir: tmpDir }, ['get', 'name', 'version'])
      const parsed = JSON.parse(result as string)
      expect(parsed.name).toBe('test-package')
      expect(parsed.version).toBe('1.0.0')
      expect(parsed.description).toBeUndefined()
    })

    test('gets nested keys using dot notation', async () => {
      const manifest = {
        name: 'test-package',
        scripts: { build: 'tsc', test: 'jest' },
      }
      fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

      const result = await handler({ dir: tmpDir }, ['get', 'scripts.build'])
      const parsed = JSON.parse(result as string)
      expect(parsed['scripts.build']).toBe('tsc')
    })
  })

describe('set subcommand', () => {
  test('sets a simple key-value pair', async () => {
    const manifest = { name: 'test-package' }
    fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

    await handler({ dir: tmpDir }, ['set', 'version=1.0.0'])
    const updated = JSON.parse(fs.readFileSync(path.join(tmpDir, 'package.json'), 'utf8'))
    expect(updated.version).toBe('1.0.0')
  })

  test('sets nested keys using dot notation', async () => {
    const manifest = { name: 'test-package' }
    fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

    await handler({ dir: tmpDir }, ['set', 'scripts.build=tsc'])
    const updated = JSON.parse(fs.readFileSync(path.join(tmpDir, 'package.json'), 'utf8'))
    expect(updated.scripts.build).toBe('tsc')
  })

  test('sets multiple key-value pairs', async () => {
    const manifest = { name: 'test-package' }
    fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

    await handler({ dir: tmpDir }, ['set', 'version=1.0.0', 'description=A test'])
    const updated = JSON.parse(fs.readFileSync(path.join(tmpDir, 'package.json'), 'utf8'))
    expect(updated.version).toBe('1.0.0')
    expect(updated.description).toBe('A test')
  })

  test('sets JSON values with --json flag', async () => {
    const manifest = { name: 'test-package' }
    fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

    await handler({ dir: tmpDir, json: true }, ['set', 'version=2'])
    const updated = JSON.parse(fs.readFileSync(path.join(tmpDir, 'package.json'), 'utf8'))
    expect(updated.version).toBe(2)
  })

  test('throws error for invalid key=value format', async () => {
    const manifest = { name: 'test-package' }
    fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

    await expect(handler({ dir: tmpDir }, ['set', 'invalidformat'])).rejects.toThrow()
  })
})

describe('delete subcommand', () => {
  test('deletes a key from package.json', async () => {
    const manifest = {
      name: 'test-package',
      version: '1.0.0',
      description: 'To be deleted',
    }
    fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

    await handler({ dir: tmpDir }, ['delete', 'description'])
    const updated = JSON.parse(fs.readFileSync(path.join(tmpDir, 'package.json'), 'utf8'))
    expect(updated.description).toBeUndefined()
    expect(updated.name).toBe('test-package')
  })

  test('deletes nested keys using dot notation', async () => {
    const manifest = {
      name: 'test-package',
      scripts: { build: 'tsc', test: 'jest' },
    }
    fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

    await handler({ dir: tmpDir }, ['delete', 'scripts.test'])
    const updated = JSON.parse(fs.readFileSync(path.join(tmpDir, 'package.json'), 'utf8'))
    expect(updated.scripts.test).toBeUndefined()
    expect(updated.scripts.build).toBe('tsc')
  })

  test('throws error when no keys provided', async () => {
    const manifest = { name: 'test-package' }
    fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

    await expect(handler({ dir: tmpDir }, ['delete'])).rejects.toThrow()
  })
})

  describe('fix subcommand', () => {
    test('fixes invalid name field', async () => {
      const manifest = {
        name: 123 as any,
        version: '1.0.0',
      }
      fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

      await handler({ dir: tmpDir }, ['fix'])
      const updated = JSON.parse(fs.readFileSync(path.join(tmpDir, 'package.json'), 'utf8'))
      expect(updated.name).toBeUndefined()
      expect(updated.version).toBe('1.0.0')
    })

    test('fixes invalid dependencies field', async () => {
      const manifest = {
        name: 'test-package',
        dependencies: 'invalid' as any,
      }
      fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

      await handler({ dir: tmpDir }, ['fix'])
      const updated = JSON.parse(fs.readFileSync(path.join(tmpDir, 'package.json'), 'utf8'))
      expect(updated.dependencies).toBeUndefined()
    })
  })

describe('error handling', () => {
  test('throws error for unknown subcommand', async () => {
    const manifest = { name: 'test-package' }
    fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

    await expect(handler({ dir: tmpDir }, ['unknown'])).rejects.toThrow()
  })

  test('throws error when no subcommand provided', async () => {
    await expect(handler({ dir: tmpDir }, [])).rejects.toThrow()
  })
})

  describe('cliOptionsTypes', () => {
    test('returns correct option types', () => {
      const types = cliOptionsTypes()
      expect(types).toHaveProperty('dir')
      expect(types).toHaveProperty('json')
      expect(types).toHaveProperty('workspace')
      expect(types).toHaveProperty('workspaces')
      expect(types).toHaveProperty('ws')
    })
  })
})
