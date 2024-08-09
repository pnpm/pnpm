import assert from 'assert/strict'
import execa from 'execa'
import fs from 'fs'
import glob from 'fast-glob'
import normalizePath from 'normalize-path'
import path from 'path'
import readYamlFile from 'read-yaml-file'

const repoRoot = path.resolve(__dirname, '../../../')
const typeCheckDir = path.resolve(__dirname, '../../../__typecheck__')
const typingsDir = path.resolve(__dirname, '../../../__typings__')
const workspaceFile = path.resolve(repoRoot, 'pnpm-workspace.yaml')

interface Workspace {
  packages: string[]
}

async function main (): Promise<void> {
  process.chdir(repoRoot)

  const workspace = await readYamlFile<Workspace>(workspaceFile)
  const patterns = workspace.packages
    .map(pattern => pattern.trim())
    .filter(pattern => !pattern.startsWith('!'))
    .flatMap(pattern => [pattern, `${pattern}/test`])
    .map(pattern => `${pattern}/tsconfig.json`)
  const tsconfigFiles = await glob(patterns, {
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
      .filter(projectPath => !projectPath.includes('__typecheck__'))
      .filter(projectPath => !projectPath.includes('__utils__/tsconfig'))
      .filter(projectPath => !projectPath.startsWith('pnpm/'))
      .map(projectPath => path.resolve(projectPath))
      .map(projectPath => path.relative(typeCheckDir, projectPath))
      .map(projectPath => normalizePath(projectPath))
      .map(path => ({ path })),
  }
  fs.writeFileSync(
    path.join(typeCheckDir, 'tsconfig.json'),
    JSON.stringify(typeCheckTSConfig, undefined, 2)
  )

  await execa('tsc', ['--build'], {
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
