import { transformWasm } from './encode-wat.mjs'

export async function instrumentAtomicWaits (bytes) {
  const module = await WebAssembly.compile(bytes)
  if (WebAssembly.Module.imports(module).some(entry => entry.module === 'pnpm_atomic')) {
    return Uint8Array.from(bytes)
  }
  return transformWasm(bytes, replaceAtomicWait)
}

function replaceAtomicWait (line, index) {
  if (index === 0) return `${line}\n${imports()}`
  if (!line.trimStart().startsWith('memory.atomic.wait')) return line
  const match = /^(\s*)memory\.atomic\.wait(32|64)(?: offset=(\d+))?(?: align=\d+)?(\)*)$/.exec(line)
  if (!match) throw new Error(`Unsupported atomic wait instruction: ${line.trim()}`)
  const [, indent, width, offset = '0', closing] = match
  if (BigInt(offset) > 0xffffffffn) throw new Error(`Atomic wait offset exceeds wasm32: ${offset}`)
  return `${indent}i32.const ${offset}\n${indent}call $pnpm_cancellable_wait${width}${closing}`
}

function imports () {
  return [32, 64].map(width =>
    `  (import "pnpm_atomic" "wait${width}" (func $pnpm_cancellable_wait${width} (param i32 i${width} i64 i32) (result i32)))`,
  ).join('\n')
}
