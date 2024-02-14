#include "kernel/types.h"
#include "user/user.h"

int
main(void)
{
  int p2c_pipe[2], c2p_pipe[2];
  if (pipe(p2c_pipe) || pipe(c2p_pipe))
    exit(1);

  const int ret = fork();
  if (ret == -1)
    exit(1);
  const int pid = getpid();
  if (ret) {
    close(p2c_pipe[0]);
    close(c2p_pipe[1]);

    char c = 'i';
    if (write(p2c_pipe[1], &c, sizeof(c)) != sizeof(c))
      exit(1);
    close(p2c_pipe[1]);

    if (read(c2p_pipe[0], &c, sizeof(c)) != sizeof(c) || c != 'o')
      exit(1);
    printf("%d: received pong\n", pid);
    close(c2p_pipe[0]);
  } else {
    close(p2c_pipe[1]);
    close(c2p_pipe[0]);

    char c = '\0';
    if (read(p2c_pipe[0], &c, sizeof(c)) != sizeof(c) || c != 'i')
      exit(1);
    printf("%d: received ping\n", pid);
    close(p2c_pipe[0]);

    c = 'o';
    if (write(c2p_pipe[1], &c, sizeof(c)) != sizeof(c))
      exit(1);
    close(c2p_pipe[1]);
  }

  exit(0);
}
