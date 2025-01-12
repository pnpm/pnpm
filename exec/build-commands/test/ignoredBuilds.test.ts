import path from 'path'
import fs from 'fs'
import { ignoredBuilds } from '@pnpm/exec.build-commands'
import { tempDir } from '@pnpm/prepare-temp-dir'
import { writeModulesManifest } from '@pnpm/modules-yaml'

test('ignoredBuilds lists automatically ignored dependencies', async () => {
  const dir = tempDir()
  const modulesDir = path.join(dir, 'node_modules')
  fs.mkdirSync(modulesDir, { recursive: true })
  await writeModulesManifest(modulesDir, {
    ignoredBuilds: ['foo'],
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
  })
  const output = await ignoredBuilds.handler({
    dir,
    modulesDir,
    rootProjectManifest: {},
  })
  expect(output).toMatchSnapshot()
})
