// Preloaded with DYLD_INSERT_LIBRARIES to count fcntl(F_FULLFSYNC) calls.
// FULLFSYNC_MODE=noop turns each call into a no-op. Each call appends
// "<pid> <milliseconds>" to the file named by FULLFSYNC_LOG.
#include <fcntl.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

static double now_ms(void) {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return t.tv_sec * 1e3 + t.tv_nsec / 1e6;
}

static int counted_fcntl(int fd, int cmd, ...) {
  va_list ap;
  va_start(ap, cmd);
  void *arg = va_arg(ap, void *);
  va_end(ap);
  if (cmd != F_FULLFSYNC) return fcntl(fd, cmd, arg);

  const char *mode = getenv("FULLFSYNC_MODE");
  double start = now_ms();
  int result = (mode != NULL && strcmp(mode, "noop") == 0) ? 0 : fcntl(fd, cmd, arg);
  const char *log = getenv("FULLFSYNC_LOG");
  if (log != NULL) {
    char line[64];
    int len = snprintf(line, sizeof line, "%d %.3f\n", getpid(), now_ms() - start);
    int log_fd = open(log, O_WRONLY | O_APPEND | O_CREAT, 0644);
    if (log_fd >= 0) {
      write(log_fd, line, (size_t)len);
      close(log_fd);
    }
  }
  return result;
}

__attribute__((used)) static struct {
  const void *replacement;
  const void *original;
} interposers[] __attribute__((section("__DATA,__interpose"))) = {
    {(const void *)counted_fcntl, (const void *)fcntl},
};
