/* Small C sample for gdb-memviz phase1. Build with: gcc -g phase1_sample.c -o phase1_sample */
#include <stdio.h>
#include <string.h>

struct Node {
    int id;
    int count;
    char name[16];
    struct Node *next;
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
    struct Node node0 = {0, 10, "node0", NULL};
    struct Node node1 = {1, 20, "node1", NULL};
    struct Node node2 = {2, 30, "node2", NULL};

    node0.next = &node1;
    node1.next = &node2;
    node2.next = NULL;

    struct Node *node_ptr = &node0;
    int *p = &arr[3];

    helper(x, node_ptr);
    helper(y, node_ptr->next);
    *p = x + y;
    arr[0] = node0.count + node1.count;

    printf("main: x=%d y=%d arr[0]=%d p=%d name0=%s name1=%s\n", x, y, arr[0], *p, node0.name, node1.name);
    return 0;
}
