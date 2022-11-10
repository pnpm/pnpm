import { promises as fs, writeFileSync } from 'fs'
import path from 'path'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { Lockfile } from '@pnpm/lockfile-types'
import prepare, { prepareEmpty, preparePackages } from '@pnpm/prepare'
import { fromDir as readPackageJsonFromDir } from '@pnpm/read-package-json'
import readProjectManifest from '@pnpm/read-project-manifest'
import writeProjectManifest from '@pnpm/write-project-manifest'
import dirIsCaseSensitive from 'dir-is-case-sensitive'
import readYamlFile from 'read-yaml-file'
import rimraf from '@zkochan/rimraf'
import isWindows from 'is-windows'
import loadJsonFile from 'load-json-file'
import exists from 'path-exists'
import crossSpawn from 'cross-spawn'
import * as TJS from 'typescript-json-schema'
import {
  execPnpm,
  execPnpmSync,
} from '../utils'

const skipOnWindows = isWindows() ? test.skip : test

// integration test for packages/lockfile-types/src/index.ts
test.only('pnpm-lock.yaml has valid schema', async () => {
  const project = prepare({
    dependencies: {
      rimraf: '2.6.2',
    },
  })

  await execPnpm(['install'])

  // TODO

  /* eslint-disable @typescript-eslint/no-unnecessary-type-assertion */
  expect(lockfile.packages!['/is-positive/1.0.0'].dependencies).toStrictEqual({
    'dep-of-pkg-with-1-dep': '100.1.0',
  })
  expect(lockfile.packages!['/is-negative/1.0.0'].dependencies).toStrictEqual({
    'dep-of-pkg-with-1-dep': '100.1.0',
  })
  /* eslint-enable @typescript-eslint/no-unnecessary-type-assertion */
})
