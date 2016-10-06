import crossSpawn = require('cross-spawn')

export default function runNpm (args: string[]) {
  const result = crossSpawn.sync('npm', args, { stdio: 'inherit' })
  process.exit(result.status)
}
