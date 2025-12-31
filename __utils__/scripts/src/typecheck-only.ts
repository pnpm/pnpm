import { readWorkspaceManifest } from '@pnpm/workspace.read-manifest'
import { findWorkspacePackages } from '@pnpm/workspace.find-packages'
import assert from 'assert/strict'
import { sync as execa } from 'execa'
import fs from 'fs'
import glob from 'fast-glob'
import normalizePath from 'normalize-path'
import path from 'path'

const repoRoot = path.resolve(import.meta.dirname, '../../../')
const typeCheckDir = path.resolve(repoRoot, '__typecheck__')
const typingsDir = path.resolve(import.meta.dirname, '__typings__')

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

  execa('tsc', ['--build', typeCheckDir], {
    // The INIT_CWD variable is populated by package managers and points towards
    // the user's original working directory. It's more useful to run TypeScript
    // from the user's actual working directory so any type checking errors can
    // reference files relative to the real CWD. This allows better integration
    // with terminals. For example, most terminals support Ctrl+Click or
    // Cmd+Click on file paths printed to directly open them.
    //
    // There's intentionally no fallback if INIT_CWD is undefined. In that case,
    // this script isn't run through a package manager and we can allow the
    // current working directory used to be the user's real one.
    cwd: process.env.INIT_CWD,
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
