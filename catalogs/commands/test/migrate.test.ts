import { fixtures } from '@pnpm/test-fixtures'
import { jest } from '@jest/globals'
import { loadJsonFileSync } from 'load-json-file'
import { sync as readYamlFile } from 'read-yaml-file'

const f = fixtures(import.meta.dirname)

jest.unstable_mockModule('enquirer', () => ({ default: { prompt: jest.fn() } }))
const { default: enquirer } = await import('enquirer')
const  { catalog } = await import('@pnpm/catalogs.commands')

const prompt = jest.mocked(enquirer.prompt)

test('migrate', async () => {
  const migrateFixture = f.prepare('migrate')
  process.chdir(migrateFixture)

  await catalog.subcommands
    .find((cmd) => cmd.commandNames.includes('migrate'))!
    .handler({ interactive: false, cliOptions: {
      argv: [],
      dir: process.cwd(),
    }}, [])

  const manifest = readYamlFile<{ catalog?: Record<string, string> }>('pnpm-workspace.yaml')

  expect(manifest.catalog).toBeDefined()
  expect(manifest.catalog?.['is-odd']).toBe('1.0.0')
  expect(manifest.catalog?.['is-zero']).toBe('1.0.0')
  expect(manifest.catalog?.['is-positive']).toBeUndefined()
  expect(manifest.catalog?.['is-negative']).toBeUndefined()
  expect(manifest.catalog?.['is-even']).toBeUndefined()

  expect(loadJsonFileSync<{ dependencies: Record<string, string>}>('packages/foo/package.json').dependencies).toEqual({
    'is-positive': '1.0.0',
    'is-negative': '1.0.1',
    'is-odd': 'catalog:',
    'is-even': '0.1.0',
  })

  expect(loadJsonFileSync<{ dependencies: Record<string, string>}>('packages/bar/package.json').dependencies).toEqual({
    'is-positive': '3.1.0',
    'is-negative': '1.0.0',
    'is-odd': 'catalog:',
    'is-even': '1.0.0',
    'is-zero': 'catalog:',
  })
})

test('interactive migrate', async () => {
  const migrateFixture = f.prepare('migrate')
  process.chdir(migrateFixture)

  prompt.mockResolvedValueOnce({
    value: {
      'is-zero@1.0.0': ['is-zero', '1.0.0'],
      'is-positive@3.1.0': ['is-positive', '3.1.0'],
      'is-negative@1.0.1': ['is-negative', '1.0.1'],
    },
  })

  await catalog.subcommands
    .find((cmd) => cmd.commandNames.includes('migrate'))!
    .handler({ interactive: true, cliOptions: {
      argv: [],
      dir: process.cwd(),
    }}, [])

  const manifest = readYamlFile<{ catalog?: Record<string, string> }>('pnpm-workspace.yaml')

  expect(manifest.catalog).toBeDefined()
  expect(manifest.catalog?.['is-zero']).toBe('1.0.0')
  expect(manifest.catalog?.['is-positive']).toBe('3.1.0')
  expect(manifest.catalog?.['is-negative']).toBe('1.0.1')

  expect(loadJsonFileSync<{ dependencies: Record<string, string>}>('packages/foo/package.json').dependencies).toEqual({
    'is-positive': 'catalog:',
    'is-negative': 'catalog:',
    'is-odd': '1.0.0',
    'is-even': '0.1.0',
  })

  expect(loadJsonFileSync<{ dependencies: Record<string, string>}>('packages/bar/package.json').dependencies).toEqual({
    'is-positive': 'catalog:',
    'is-negative': 'catalog:',
    'is-odd': '1.0.0',
    'is-even': '1.0.0',
    'is-zero': 'catalog:',
  })
})