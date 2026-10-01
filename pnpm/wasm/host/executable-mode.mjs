export function hasExecutableMode (metadata) {
  const user = process.getuid()
  let permission = 0o1
  if (user === 0) permission = 0o111
  else if (metadata.uid === user) permission = 0o100
  else if (metadata.gid === process.getgid() || process.getgroups().includes(metadata.gid)) permission = 0o10
  return (metadata.mode & permission) !== 0
}
