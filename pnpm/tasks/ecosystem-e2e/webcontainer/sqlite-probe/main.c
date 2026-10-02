#include "sqlite3.h"
#include <stdio.h>
#include <string.h>
#include <unistd.h>

static int print_result(void *context, int columns, char **values, char **names) {
  (void)context;
  for (int index = 0; index < columns; index++) {
    printf("%s=%s\n", names[index], values[index] ? values[index] : "NULL");
  }
  return 0;
}

static int execute(sqlite3 *database, const char *sql) {
  char *error = NULL;
  int result = sqlite3_exec(database, sql, print_result, NULL, &error);
  if (result != SQLITE_OK) fprintf(stderr, "SQLite %d: %s\n", result, error);
  sqlite3_free(error);
  return result;
}

int main(int argc, char **argv) {
  if (argc != 3) {
    fprintf(stderr, "usage: sqlite-probe <database> <write|read|hold>\n");
    return 1;
  }
  sqlite3 *database = NULL;
  int result = sqlite3_open(argv[1], &database);
  if (result != SQLITE_OK) {
    fprintf(stderr, "SQLite open %s: %s\n", argv[1], sqlite3_errmsg(database));
    sqlite3_close(database);
    return 1;
  }
  printf("threadsafe=%d\n", sqlite3_threadsafe());
  result = execute(database, "PRAGMA busy_timeout=1000; PRAGMA temp_store=MEMORY;");
  if (result == SQLITE_OK && strcmp(argv[2], "read") == 0) {
    result = execute(database, "PRAGMA integrity_check; PRAGMA journal_mode; SELECT count(*) AS rows FROM probe;");
  } else if (result == SQLITE_OK && (strcmp(argv[2], "write") == 0 || strcmp(argv[2], "hold") == 0)) {
    result = execute(database, "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA mmap_size=536870912; CREATE TABLE IF NOT EXISTS probe (value TEXT); BEGIN IMMEDIATE; INSERT INTO probe VALUES ('persisted');");
    if (result == SQLITE_OK) {
      if (strcmp(argv[2], "hold") == 0) {
        puts("transaction-held");
        fflush(stdout);
        sleep(3);
      }
      result = execute(database, "COMMIT;");
    }
  } else if (result == SQLITE_OK) {
    fprintf(stderr, "unknown probe operation: %s\n", argv[2]);
    result = SQLITE_ERROR;
  }
  sqlite3_close(database);
  return result == SQLITE_OK ? 0 : 1;
}
