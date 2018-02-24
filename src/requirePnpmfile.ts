import chalk from 'chalk'
import path = require('path')

export default (prefix: string) => {
  try {
    const pnpmFilePath = path.join(prefix, 'pnpmfile.js')
    return require(pnpmFilePath)
  } catch (err) {
    if (err instanceof SyntaxError) {
      console.error(chalk.red('A syntax error in the pnpmfile.js\n'))
      console.error(err)
      process.exit(1)
      return
    }
    if (err.code !== 'MODULE_NOT_FOUND') throw err
    return {}
  }
}
