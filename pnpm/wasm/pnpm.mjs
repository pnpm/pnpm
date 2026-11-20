#!/usr/bin/env node
import { fileURLToPath } from 'node:url'

import { runWasm } from './run.mjs'

const executable = fileURLToPath(import.meta.url)
const artifact = fileURLToPath(new URL('./pnpm.wasm', import.meta.url))
process.exitCode = await runWasm(artifact, { executable, args: [executable, ...process.argv.slice(2)] })
