import {
  progressLogger,
  ProgressMessage,
} from '../loggers'

export default (loginfo: ProgressMessage) => progressLogger.debug(loginfo)
