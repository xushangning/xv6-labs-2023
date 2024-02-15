#include "kernel/types.h"
#include "user/user.h"

int
filter_divisibles(const int in_fd)
{
  int p;
  if (read(in_fd, &p, sizeof(p)) != sizeof(p))
    return 1;
  printf("prime %d\n", p);

  int the_pipe[2];
  if (pipe(the_pipe) == -1)
    return 1;
  int ret = fork();
  if (ret == -1) {
    close(the_pipe[0]);
    close(the_pipe[1]);
    return 1;
  }
  if (ret) {
    close(the_pipe[0]);
    int n;
    while ((ret = read(in_fd, &n, sizeof(n))) == sizeof(n)
      && (n % p == 0 || write(the_pipe[1], &n, sizeof(n)) == sizeof(n)))
      ;

    close(the_pipe[1]);
    if (ret)
      return 1;
    wait(&ret);
  } else {
    close(in_fd);
    close(the_pipe[1]);
    ret = filter_divisibles(the_pipe[0]);
    close(the_pipe[0]);
  }

  return ret;
}

int
main(void)
{
  int the_pipe[2];
  if (pipe(the_pipe) == -1)
    exit(1);
  int ret = fork();
  if (ret == -1)
    exit(1);
  if (ret) {
    close(the_pipe[0]);
    for (int i = 2; i <= 35; ++i)
      if (write(the_pipe[1], &i, sizeof(i)) != sizeof(i))
        exit(1);
    close(the_pipe[1]);
    wait(&ret);
  } else {
    close(the_pipe[1]);
    ret = filter_divisibles(the_pipe[0]);
    close(the_pipe[0]);
  }

  exit(ret);
}
