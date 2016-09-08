import expandTilde from './fs/expand_tilde'

export default globalPath => expandTilde(globalPath || '~/.pnpm')
