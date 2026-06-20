for (const [key, value] of Object.entries(process.env)) {
  if (key.startsWith('npm_config_') && key !== 'npm_config_node_gyp') {
    process.stdout.write(`${key}=${value}\n`)
  }
}
