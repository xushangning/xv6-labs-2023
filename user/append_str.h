#ifndef APPEND_STR_H
#define APPEND_STR_H

int expand_str(char** s, uint* cap, const uint target)
{
    uint new_cap = *cap;
    do
        new_cap *= 2;
    while (new_cap < target);
    char* const new_s = malloc(new_cap);
    if (new_s == 0)
        return 0;

    memcpy(new_s, *s, *cap);
    free(*s);
    *s = new_s;
    *cap = new_cap;
    return 1;
}

int append_char(char** s, uint* size, uint* cap, char c)
{
    const uint new_size = *size + 1;
    if (new_size > *cap)
        if (!expand_str(s, cap, new_size))
            return 0;

    (*s)[*size - 1] = c;
    (*s)[(*size)++] = '\0';
    return 1;
}

int append_str(char** s, uint* size, uint* cap, const char* append)
{
    const uint append_size = strlen(append),
        new_size = *size + append_size;
    if (new_size > *cap)
        if (!expand_str(s, cap, new_size))
            return 0;

    // append_size + 1 to copy over the null terminator.
    memcpy(*s + *size - 1, append, append_size + 1);
    *size = new_size;
    return 1;
}

#endif //APPEND_STR_H
