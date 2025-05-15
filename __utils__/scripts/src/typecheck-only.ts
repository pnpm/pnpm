import { readWorkspaceManifest } from '@pnpm/workspace.read-manifest'
import { findWorkspacePackages } from '@pnpm/workspace.find-packages'
import assert from 'assert/strict'
import { sync as execa } from 'execa'
import fs from 'fs'
import glob from 'fast-glob'
import normalizePath from 'normalize-path'
import path from 'path'

const repoRoot = path.resolve(__dirname, '../../../')
const typeCheckDir = path.resolve(repoRoot, '__typecheck__')
const typingsDir = path.resolve(__dirname, '__typings__')

async function main (): Promise<void> {
  const workspace = await readWorkspaceManifest(repoRoot)
  const packages = await findWorkspacePackages(repoRoot, {
    patterns: workspace!.packages,
  })
  const patterns = packages
    .map(({ rootDir }) => normalizePath(path.relative(repoRoot, rootDir)))
    .flatMap(rootDir => [`${rootDir}/tsconfig.json`, `${rootDir}/test/tsconfig.json`])
  const tsconfigFiles = await glob(patterns, {
    cwd: repoRoot,
    onlyFiles: true,
  })
  assert.notEqual(tsconfigFiles.length, 0)

  const typeCheckTSConfig = {
    extends: '@pnpm/tsconfig',
    compilerOptions: {
      composite: false,
      rootDir: '.',
      outDir: 'lib',
      declaration: false,
    },
    include: [
      `${normalizePath(path.relative(typeCheckDir, typingsDir))}/**/*.d.ts`,
    ],
    exclude: [
      path.relative(typeCheckDir, repoRoot),
    ],
    references: tsconfigFiles
      .filter(projectPath => {
        return !projectPath.includes('__typecheck__') &&
          !projectPath.includes('__utils__/tsconfig')
      })
      .map(projectPath => ({
        path: normalizePath(path.relative(typeCheckDir, projectPath)),
      })),
  }
  fs.writeFileSync(
    path.join(typeCheckDir, 'tsconfig.json'),
    JSON.stringify(typeCheckTSConfig, undefined, 2)
  )

  execa('tsc', ['--build'], {
    cwd: typeCheckDir,
    stdio: 'inherit',
  })
}

main().catch((error: unknown) => {
  if (error && typeof error === 'object' && 'exitCode' in error && 'shortMessage' in error) {
    process.exit(error.exitCode as number)
  } else {
    console.error(error)
    process.exit(1)
  }
})
