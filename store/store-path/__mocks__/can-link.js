const CAN_LINK = new Set([
  '/can-link-to-homedir/tmp=>/home/user/tmp',
  '/mnt/project/tmp=>/mnt/tmp/tmp',
])

module.exports = function (existingPath, newPath) {
  return CAN_LINK.has(`${existingPath}=>${newPath}`)
}
