import path from 'path'
import fs from 'fs'
import { ignoredBuilds } from '@pnpm/exec.build-commands'
import { tempDir } from '@pnpm/prepare-temp-dir'
import { writeModulesManifest } from '@pnpm/modules-yaml'

const DEFAULT_MODULES_MANIFEST = {
  hoistedDependencies: {},
  layoutVersion: 4,
  packageManager: '',
  included: {
    optionalDependencies: true,
    dependencies: true,
    devDependencies: true,
  },
  pendingBuilds: [],
  prunedAt: '',
  skipped: [],
  storeDir: '',
  virtualStoreDir: '',
  virtualStoreDirMaxLength: 90,
  registries: {
    default: '',
  },
}

test('ignoredBuilds lists automatically ignored dependencies', async () => {
  const dir = tempDir()
  const modulesDir = path.join(dir, 'node_modules')
  fs.mkdirSync(modulesDir, { recursive: true })
  await writeModulesManifest(modulesDir, {
    ...DEFAULT_MODULES_MANIFEST,
    ignoredBuilds: ['foo'],
  })
  const output = await ignoredBuilds.handler({
    dir,
    modulesDir,
    rootProjectManifest: {},
  })
  expect(output).toMatchSnapshot()
})

test('ignoredBuilds lists explicitly ignored dependencies', async () => {
  const dir = tempDir()
  const modulesDir = path.join(dir, 'node_modules')
  fs.mkdirSync(modulesDir, { recursive: true })
  await writeModulesManifest(modulesDir, {
    ...DEFAULT_MODULES_MANIFEST,
    ignoredBuilds: [],
  })
  const output = await ignoredBuilds.handler({
    dir,
    modulesDir,
    rootProjectManifest: {
      pnpm: {
        ignoredBuiltDependencies: ['bar'],
      },
    },
  })
  expect(output).toMatchSnapshot()
})

test('ignoredBuilds lists both automatically and explicitly ignored dependencies', async () => {
  const dir = tempDir()
  const modulesDir = path.join(dir, 'node_modules')
  fs.mkdirSync(modulesDir, { recursive: true })
  await writeModulesManifest(modulesDir, {
    ...DEFAULT_MODULES_MANIFEST,
    ignoredBuilds: ['foo', 'bar'],
  })
  const output = await ignoredBuilds.handler({
    dir,
    modulesDir,
    rootProjectManifest: {
      pnpm: {
        ignoredBuiltDependencies: ['qar', 'zoo'],
      },
    },
  })
  expect(output).toMatchSnapshot()
})

test('ignoredBuilds prints an info message when there is no node_modules', async () => {
  const dir = tempDir()
  const modulesDir = path.join(dir, 'node_modules')
  const output = await ignoredBuilds.handler({
    dir,
    modulesDir,
    rootProjectManifest: {
      pnpm: {
        ignoredBuiltDependencies: ['qar', 'zoo'],
      },
    },
  })
  expect(output).toMatchSnapshot()
})
