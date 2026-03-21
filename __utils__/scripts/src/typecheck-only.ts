import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { findWorkspaceProjects } from '@pnpm/workspace.projects-reader'
import { readWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-reader'
import { sync as execa } from 'execa'
import glob from 'fast-glob'
import normalizePath from 'normalize-path'

const repoRoot = path.resolve(import.meta.dirname, '../../../')
const typeCheckDir = path.resolve(repoRoot, '__typecheck__')
const typingsDir = path.resolve(import.meta.dirname, '__typings__')

async function main (): Promise<void> {
  const workspace = await readWorkspaceManifest(repoRoot)
  const packages = await findWorkspaceProjects(repoRoot, {
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

  const singleThreaded = resolveThreadingMode(repoRoot)
  const args = ['--build']
  if (singleThreaded) {
    args.push('--singleThreaded')
  }
  args.push(typeCheckDir)
  console.log(`Running tsgo --build${singleThreaded ? ' --singleThreaded' : ''}...`)
  execa('tsgo', args, {
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
  console.log('Running tsgo build done')
}

const BYTES_PER_GB = 1024 ** 3
const AUTO_SINGLE_THREAD_MEMORY_THRESHOLD_GB = 8

function resolveThreadingMode (repoRoot: string): boolean {
  const { mode, source } = readThreadingMode(repoRoot)
  switch (mode) {
    case 'single-threaded':
      return true
    case 'multi-threaded':
      return false
    case 'auto':
      return os.totalmem() / BYTES_PER_GB < AUTO_SINGLE_THREAD_MEMORY_THRESHOLD_GB
    default:
      throw new Error(
        `Invalid threading mode "${mode}" from ${source}. ` +
        'Valid values: auto, single-threaded, multi-threaded.'
      )
  }
}

function readThreadingMode (repoRoot: string): { mode: string, source: string } {
  const envValue = process.env.PNPM_TYPECHECK_THREADING?.trim().toLowerCase()
  if (envValue) {
    return { mode: envValue, source: 'PNPM_TYPECHECK_THREADING env var' }
  }

  for (const configPath of [
    path.join(repoRoot, '.local-settings', 'pnpm-typecheck.json'),
    path.join(repoRoot, '.pnpm-typecheck.json'),
  ]) {
    if (fs.existsSync(configPath)) {
      const config = JSON.parse(fs.readFileSync(configPath, 'utf-8'))
      const threading = typeof config.threading === 'string' ? config.threading.trim().toLowerCase() : ''
      if (threading) {
        return { mode: threading, source: configPath }
      }
    }
  }

  return { mode: 'auto', source: 'default' }
}

main().catch((error: unknown) => {
  if (error && typeof error === 'object' && 'exitCode' in error && 'shortMessage' in error) {
    process.exit(error.exitCode as number)
  } else {
    console.error(error)
    process.exit(1)
  }
})
