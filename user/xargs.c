#include <kernel/param.h>
#include <kernel/types.h>
#include <user/user.h>

#include "append_str.h"

int main(const int argc, char* argv[])
{
    if (argc < 2)
    {
        fprintf(2, "Usage: %s <command> [<arg> ...]", argv[0]);
        exit(0);
    }

    char *command = argv[1], *new_argv[MAXARG];
    // + 1 for the null terminator.
    if (argc + 1 > MAXARG)
    {
        fprintf(2, "%s: Too many (%d) arguments\n", __FUNCTION__, argc);
    }
    memcpy(new_argv, argv + 1, sizeof(*argv) * (argc - 1));

    uint cap = 32, size = 1;
    char c, *s = malloc(cap);
    if (s == 0)
    {
        fprintf(2, "%s: Cannot allocate space for s\n", __FUNCTION__);
        exit(0);
    }
    *s = '\0';

    while (read(0, &c, 1) == 1)
    {
        if (c != '\n')
        {
            if (append_char(&s, &size, &cap, c) == 0)
            {
                fprintf(2, "%s: Cannot append to string\n", __FUNCTION__);
                break;
            }
            continue;
        }

        const int pid = fork();
        if (pid < 0)
        {
            fprintf(2, "%s: Failed fork\n", __FUNCTION__);
            break;
        }
        if (pid > 0)
        {
            int status;
            wait(&status);
        }
        else
        {
            new_argv[argc - 1] = s;
            new_argv[argc] = 0;
            if (exec(command, new_argv) < 0)
            {
                fprintf(2, "%s: Failed exec\n", __FUNCTION__);
                break;
            }
        }

        size = 1;
        *s = '\0';
    }

    free(s);
    exit(0);
}
