import logger, {ProgressMessage, progressLogger} from 'pnpm-logger'

export default (loginfo: ProgressMessage) => progressLogger.debug(loginfo)
