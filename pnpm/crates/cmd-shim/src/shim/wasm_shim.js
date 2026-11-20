(async () => {
  const fs = await import('node:fs')
  const path = await import('node:path')
  const { spawn } = await import('node:child_process')
  const config = PNPM_SHIM_CONFIG
  const directory = path.dirname(fs.realpathSync(process.argv[1]))
  const target = path.resolve(directory, config.target)
  if (config.arguments.Err) throw new Error(config.arguments.Err)
  const nodePath = config.nodePath.map(entry => path.resolve(directory, entry))
  if (nodePath.length) process.env.NODE_PATH = [...nodePath, process.env.NODE_PATH].filter(Boolean).join(':')
  let program = config.program
  if (program && !path.isAbsolute(program)) {
    const local = path.resolve(directory, program)
    try {
      fs.accessSync(local, fs.constants.X_OK)
      if (hasExecutableMode(fs.statSync(local))) program = local
    } catch (error) {
      if (error.code !== 'ENOENT' && error.code !== 'EACCES') throw error
    }
  }
  const args = [...config.arguments.Ok, ...(program ? [target] : []), ...process.argv.slice(2)]
  const child = spawn(program || target, args, { stdio: 'inherit' })
  for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) process.on(signal, () => child.kill(signal))
  child.on('error', error => { console.error(error.message); process.exitCode = 1 })
  child.on('exit', (code, signal) => {
    if (signal) {
      process.removeAllListeners(signal)
      process.kill(process.pid, signal)
    } else {
      process.exitCode = code
    }
  })
})().catch(error => { console.error(error); process.exitCode = 1 })
