#!/usr/bin/env node
import { mkdir, writeFile } from 'node:fs/promises'
import { dirname } from 'node:path'
import { spawn } from 'node:child_process'
import { performance } from 'node:perf_hooks'
import { resolveBenchOutputPath } from './bench-output-path.mjs'

const { name, output, command } = parseArgs(process.argv.slice(2))
const outputPath = resolveBenchOutputPath(output)

await mkdir(dirname(outputPath), { recursive: true })

const startedAt = performance.now()
const exitCode = await runCommand(command)
const durationSeconds = (performance.now() - startedAt) / 1000

await writeFile(outputPath, JSON.stringify({
  results: [
    {
      command: name,
      mean: durationSeconds,
      stddev: 0,
      median: durationSeconds,
      user: 0,
      system: 0,
      min: durationSeconds,
      max: durationSeconds,
      times: [durationSeconds],
      exit_codes: [exitCode],
    },
  ],
}, null, 2) + '\n')

process.exitCode = exitCode

function parseArgs (args) {
  let name
  let output
  const commandIndex = args.indexOf('--')

  if (commandIndex === -1) {
    usage('missing command separator: --')
  }

  for (let i = 0; i < commandIndex; i++) {
    const arg = args[i]
    if (arg === '--name') {
      name = args[++i]
    } else if (arg === '--output') {
      output = args[++i]
    } else {
      usage(`unknown argument: ${arg}`)
    }
  }

  const command = args.slice(commandIndex + 1)
  if (!name) usage('missing --name')
  if (!output) usage('missing --output')
  if (command.length === 0) usage('missing command')

  return { name, output, command }
}

function usage (message) {
  console.error(message)
  console.error('Usage: measure-command.mjs --name <benchmark> --output <file> -- <command> [args...]')
  process.exit(1)
}

function runCommand ([command, ...args]) {
  return new Promise((resolve) => {
    const shell = process.platform === 'win32'
    if (shell) {
      validateWindowsShellArgs([command, ...args])
    }
    const child = spawn(command, args, { shell, stdio: 'inherit' })
    child.on('error', (err) => {
      console.error(err)
      resolve(1)
    })
    child.on('close', (code, signal) => {
      if (signal) {
        console.error(`Command terminated by signal ${signal}`)
        resolve(1)
      } else {
        resolve(code ?? 1)
      }
    })
  })
}

function validateWindowsShellArgs (args) {
  for (const arg of args) {
    if (/[&|<>^%\r\n]/.test(arg)) {
      throw new Error(`Cannot run command with Windows shell metacharacters: ${arg}`)
    }
  }
}
