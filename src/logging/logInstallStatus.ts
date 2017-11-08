import {
  ProgressMessage,
  progressLogger,
} from '../loggers'

export default (loginfo: ProgressMessage) => progressLogger.debug(loginfo)
