/* Small C sample for gdb-memviz phase1. Build with: gcc -g phase1_sample.c -o phase1_sample */
#include <stdio.h>
#include <string.h>

struct Node {
    int id;
    int count;
    char name[16];
};

static void helper(int seed, struct Node *node) {
    int local = seed * 3;
    int helper_arr[4] = {11, 22, 33, 44};
    int *ptr = &helper_arr[2];
    node->count += local + *ptr;
    snprintf(node->name, sizeof(node->name), "id%d", node->id);
    printf("helper: local=%d ptr=%d name=%s\n", local, *ptr, node->name);
}

int main(int argc, char **argv) {
    int x = 42;
    int y = argc + 7;
    int arr[5] = {1, 2, 3, 4, 5};
    struct Node node = {7, 21, "init"};
    struct Node *node_ptr = &node;
    int *p = &arr[3];

    helper(x, node_ptr);
    *p = x + y;
    arr[0] = node.count;

    printf("main: x=%d y=%d arr[0]=%d p=%d name=%s\n", x, y, arr[0], *p, node.name);
    return 0;
}
