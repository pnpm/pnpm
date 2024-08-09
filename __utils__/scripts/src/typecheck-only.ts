import * as assert from 'assert/strict'
import * as cp from 'child_process'
import * as glob from 'fast-glob'
import * as path from 'path'
import * as pLimit from 'p-limit'
import readYamlFile from 'read-yaml-file'

const repoRoot = path.resolve(__dirname, '../../../')
const workspaceFile = path.resolve(repoRoot, 'pnpm-workspace.yaml')

interface Workspace {
  packages: string[]
}

interface TaskResult {
  project: string
  success: boolean
  stdout: string
  stderr: string
}

async function main(): Promise<void> {
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

  const limit = pLimit(50)
  const promises = tsconfigFiles.map(tsconfigFile => limit(() => new Promise<TaskResult>((resolve, reject) => {
    const project = path.dirname(tsconfigFile)
    const child = cp.spawn('tsc', ['--noEmit', '-p', tsconfigFile], {
      stdio: 'pipe',
    })
    let stdout = ''
    child.stdout.on('data', data => {
      stdout += String(data)
    })
    let stderr = ''
    child.stderr.on('data', data => {
      stderr += String(data)
    })
    child.once('close', status => {
      if (status === null) {
        return reject(new Error(`tsc exits without a status code (${project})`))
      }
      const success = status === 0
      resolve({ project, success, stdout, stderr })
    })
    child.once('error', error => reject(error))
  })))

  const taskResults = await Promise.all(promises)
  let failure = 0
  for (const { project, success, stdout, stderr } of taskResults) {
    if (success) continue
    failure += 1
    console.error(`PROJECT: ${project}`)
    if (stdout.trim()) {
      console.log(stdout)
    }
    if (stderr.trim()) {
      console.log(stderr)
    }
  }
  if (failure !== 0) {
    console.error(`${failure} projects fail typecheck`)
    process.exit(1)
  }
}

main().catch((error: unknown) => {
  if (error && typeof error === 'object' && 'exitCode' in error && 'shortMessage' in error) {
    process.exit(error.exitCode as number)
  } else {
    console.error(error)
    process.exit(1)
  }
})
