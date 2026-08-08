#!/usr/bin/env node
import { runCli } from '../lib/index.js'

try {
  process.exitCode = await runCli(process.argv.slice(2))
} catch (err) {
  console.error(err.message)
  process.exitCode = 1
}
