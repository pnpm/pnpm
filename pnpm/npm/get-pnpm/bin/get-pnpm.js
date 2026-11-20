#!/usr/bin/env node
import { runCli } from '../lib/index.js'

try {
  process.exitCode = await runCli(process.argv.slice(2))
} catch (err) {
  // Anything can be thrown, and reading `.message` off a non-Error would either
  // print `undefined` or throw again, burying the reason under a stack trace.
  console.error(err instanceof Error ? err.message : String(err))
  process.exitCode = 1
}
