import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { findWorkspaceProjects } from '@pnpm/workspace.projects-reader'
import { readWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-reader'
import { sync as execa } from 'execa'
import glob from 'fast-glob'
import normalizePath from 'normalize-path'

const repoRoot = path.resolve(import.meta.dirname, '../../../../')
const typeCheckDir = path.resolve(repoRoot, 'pnpm11/__typecheck__')
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

  writeTypeCheckTsConfig(tsconfigFiles)
  runTsgo()
}

function writeTypeCheckTsConfig (tsconfigFiles: string[]): void {
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
}

function runTsgo (): void {
  const singleThreaded = resolveThreadingMode(repoRoot)
  const args = ['--build']
  if (singleThreaded) {
    args.push('--singleThreaded')
  }
  args.push(typeCheckDir)
  console.log(`Running tsgo --build${singleThreaded ? ' --singleThreaded' : ''}...`)
  execa('tsgo', args, {
    cwd: process.env.INIT_CWD,
    stdio: 'inherit',
  })
  console.log('Running tsgo build done')
}

const AUTO_SINGLE_THREAD_MEMORY_THRESHOLD_GB = 8

function resolveThreadingMode (repoRoot: string): boolean {
  const { mode, source } = readThreadingMode(repoRoot)
  switch (mode) {
    case 'single-threaded':
      return true
    case 'multi-threaded':
      return false
    case 'auto':
      return os.totalmem() / (1024 ** 3) < AUTO_SINGLE_THREAD_MEMORY_THRESHOLD_GB
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
    // eslint-disable-next-line n/no-process-exit -- the script's exit code mirrors the failed child process
    process.exit(error.exitCode as number)
  } else {
    console.error(error)
    // eslint-disable-next-line n/no-process-exit -- a top-level script failure ends the process
    process.exit(1)
  }
})
