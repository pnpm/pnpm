import logger, {ProgressMessage} from 'pnpm-logger'

const progressLogger = logger('progress')

export default (loginfo: ProgressMessage) => progressLogger.debug(loginfo)
