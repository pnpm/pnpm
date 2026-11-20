import { execute } from '@yarnpkg/shell'

try {
  process.exitCode = await execute(process.argv[2], process.argv.slice(3))
} catch (error) {
  console.error(error.message)
  process.exitCode = 1
}
