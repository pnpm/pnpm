import errorHandler from './err'
import main from './main'

// Avoid "Possible EventEmitter memory leak detected" warnings
// because it breaks pnpm's CLI output
process.setMaxListeners(0)

// eslint-disable-next-line @typescript-eslint/no-floating-promises
;(async () => {
  try {
    const argv = process.argv.slice(2)
    await main(['dlx', ...argv])
  } catch (err: any) { // eslint-disable-line
    errorHandler(err)
  }
})()
