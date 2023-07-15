const { spawn } = require('child_process')

module.exports = () => {
  global.__SERVER__ = spawn('registry-mock', [])
}
