import { spawnSync } from 'node:child_process'
import { createReadStream, createWriteStream, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { createInterface } from 'node:readline'
import { Readable } from 'node:stream'
import { pipeline } from 'node:stream/promises'

export function validateWabt () {
  tool('wasm2wat')
  tool('wat2wasm')
}

export function encodeWat (source) {
  const temporary = mkdtempSync(join(tmpdir(), 'pnpm-wasm-'))
  try {
    const input = join(temporary, 'input.wat')
    const output = join(temporary, 'output.wasm')
    writeFileSync(input, source)
    run(tool('wat2wasm'), ['--enable-threads', '--debug-names', input, '-o', output])
    return Uint8Array.from(readFileSync(output))
  } finally {
    rmSync(temporary, { recursive: true, force: true })
  }
}

export async function transformWasm (bytes, transformLine) {
  const temporary = mkdtempSync(join(tmpdir(), 'pnpm-wasm-'))
  try {
    const input = join(temporary, 'input.wasm')
    const text = join(temporary, 'input.wat')
    const transformed = join(temporary, 'output.wat')
    const output = join(temporary, 'output.wasm')
    writeFileSync(input, bytes)
    run(tool('wasm2wat'), ['--enable-threads', '--generate-names', '--no-debug-names', input, '-o', text])
    await transformLines(text, transformed, transformLine)
    run(tool('wat2wasm'), ['--enable-threads', '--debug-names', transformed, '-o', output])
    return Uint8Array.from(readFileSync(output))
  } finally {
    rmSync(temporary, { recursive: true, force: true })
  }
}

async function transformLines (source, destination, transformLine) {
  const input = createReadStream(source)
  const lines = createInterface({ input, crlfDelay: Infinity })
  input.on('error', error => lines.emit('error', error))
  try {
    await pipeline(Readable.from(replaceLines(lines, transformLine)), createWriteStream(destination))
  } finally {
    lines.close()
    input.destroy()
  }
}

async function * replaceLines (lines, transformLine) {
  let index = 0
  for await (const line of lines) yield transformLine(line, index++) + '\n'
}

function tool (name) {
  const directory = process.env.WABT_PATH
  if (!directory) throw new Error('Set WABT_PATH to an extracted official WABT 1.0.42 directory')
  const program = join(directory, 'bin', name)
  const version = run(program, ['--version']).trim()
  if (version !== '1.0.42') throw new Error(`WABT 1.0.42 is required, found ${version}`)
  return program
}

function run (program, args) {
  const result = spawnSync(program, args, { encoding: 'utf8' })
  if (result.error) throw result.error
  if (result.status !== 0) throw new Error(`${program} failed: ${result.stderr || result.signal || result.status}`)
  return result.stdout
}
