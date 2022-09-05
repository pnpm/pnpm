module.exports = {
  hooks: {filterLog}
}

function filterLog(log) {
  return log.level === 'error' || log.level === 'warn'
}
