import path from 'path'
import normalize from 'normalize-path'

export default (lockfileDir: string, prefix: string): string => normalize(path.relative(lockfileDir, prefix)) || '.'
