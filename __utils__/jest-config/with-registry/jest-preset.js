import path from 'path'
import baseConfig from './../config.js'

export default {
  ...baseConfig,
  // Many tests change the dist tags of packages.
  // Unfortunately, this means that if two such tests will run at the same time,
  // they may break each other.
  maxWorkers: 1,
  // Recycle the test worker once its heap crosses this limit. Under
  // `--experimental-vm-modules` Jest's VM module registry is never released
  // between test files, so a long-running process climbs to Node's ~4 GB
  // old-space ceiling and dies with an out-of-memory FATAL ERROR. Setting a
  // limit is also what keeps `maxWorkers: 1` from collapsing to in-band
  // execution (see `shouldRunInBand`): a recyclable worker is spawned instead,
  // still serial, so the leaked memory is reclaimed between files without
  // reintroducing the dist-tag races `maxWorkers: 1` prevents.
  workerIdleMemoryLimit: '1500MB',
  // Force Jest to exit after globalTeardown completes.  The Verdaccio server
  // and lifecycle child-processes spawned during tests may leave ref'd handles
  // that prevent the process from exiting on its own.
  forceExit: true,
  globalSetup: path.join(import.meta.dirname, 'globalSetup.js'),
  globalTeardown: path.join(import.meta.dirname, 'globalTeardown.js'),
}
