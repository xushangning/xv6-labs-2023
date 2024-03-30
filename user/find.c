#include "kernel/types.h"
#include "user/user.h"
#include "kernel/stat.h"
#include "kernel/fcntl.h"
#include "kernel/fs.h"

#include "append_str.h"

void find(char** path, uint size, uint* cap, const char* name, const char* target)
{
    const int fd = open(*path, O_RDONLY);
    if (fd < 0)
    {
        fprintf(2, "%s: Cannot open %s\n", __FUNCTION__, *path);
        return;
    }

    struct stat st;
    if (fstat(fd, &st) < 0)
    {
        fprintf(2, "%s: Cannot stat %s\n", __FUNCTION__, *path);
        close(fd);
        return;
    }

    switch (st.type)
    {
    case T_DEVICE:
    case T_FILE:
        if (strcmp(name, target) == 0)
            printf("%s\n", *path);
        break;

    case T_DIR:
        {
            struct dirent de;
            while (read(fd, &de, sizeof(de)) == sizeof(de))
            {
                if (de.inum == 0 || strcmp(".", de.name) == 0 || strcmp("..", de.name) == 0)
                    continue;

                uint new_size = size;
                if (append_char(path, &new_size, cap, '/') == 0)
                {
                    fprintf(2, "%s: Failed to append \"/\" separator to %s\n", __FUNCTION__, *path);
                    break;
                }
                if (append_str(path, &new_size, cap, de.name) == 0)
                {
                    fprintf(2, "%s: Failed to append %s to %s\n", __FUNCTION__, de.name, *path);
                    break;
                }

                find(path, new_size, cap, de.name, target);
            }
            (*path)[size - 1] = '\0';

            break;
        }

    default:
        fprintf(2, "%s: Unexpected value (%d) for type of struct stat\n", __FUNCTION__ ,st.type);
    }

    close(fd);
}

int
main(int argc, char *argv[])
{
    if(argc != 3){
        fprintf(2, "Usage: %s <path> <file-name>\n", argv[0]);
        exit(1);
    }

    uint size = strlen(argv[1]) + 1,
        cap = 32;
    while (cap < size)
        cap *= 2;
    char* path = malloc(cap);
    if (path == 0)
    {
        fprintf(2, "%s: Failed to allocate memory for path\n", __FUNCTION__);
        exit(1);
    }
    memcpy(path, argv[1], size);

    find(&path, size, &cap, argv[1], argv[2]);

    free(path);
    exit(0);
}
