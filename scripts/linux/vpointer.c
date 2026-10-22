/* A virtual pointer for tests under a wlroots compositor (sway): reads
 * "move X Y", "press", "release" and "quit" lines on stdin and sends them
 * as a pointer device would, so a drag between two clients is real. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <wayland-client.h>
#include "wlr-virtual-pointer-unstable-v1-client-protocol.h"

static struct zwlr_virtual_pointer_manager_v1 *manager;
static struct wl_seat *seat;

static void global(void *data, struct wl_registry *registry, uint32_t name,
                   const char *interface, uint32_t version) {
    if (strcmp(interface, zwlr_virtual_pointer_manager_v1_interface.name) == 0)
        manager = wl_registry_bind(registry, name, &zwlr_virtual_pointer_manager_v1_interface, 1);
    else if (strcmp(interface, wl_seat_interface.name) == 0 && !seat)
        seat = wl_registry_bind(registry, name, &wl_seat_interface, 1);
}
static void global_remove(void *data, struct wl_registry *registry, uint32_t name) {}
static const struct wl_registry_listener listener = {global, global_remove};

static uint32_t now(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (uint32_t)(t.tv_sec * 1000 + t.tv_nsec / 1000000);
}

int main(int argc, char **argv) {
    unsigned width = argc > 2 ? (unsigned)atoi(argv[1]) : 1280;
    unsigned height = argc > 2 ? (unsigned)atoi(argv[2]) : 800;
    struct wl_display *display = wl_display_connect(NULL);
    if (!display) { fprintf(stderr, "no display\n"); return 1; }
    struct wl_registry *registry = wl_display_get_registry(display);
    wl_registry_add_listener(registry, &listener, NULL);
    wl_display_roundtrip(display);
    if (!manager) { fprintf(stderr, "no virtual pointer manager\n"); return 1; }
    struct zwlr_virtual_pointer_v1 *pointer =
        zwlr_virtual_pointer_manager_v1_create_virtual_pointer(manager, seat);
    wl_display_roundtrip(display);
    printf("ready\n");
    fflush(stdout);
    char line[128];
    while (fgets(line, sizeof line, stdin)) {
        unsigned x, y;
        if (sscanf(line, "move %u %u", &x, &y) == 2) {
            zwlr_virtual_pointer_v1_motion_absolute(pointer, now(), x, y, width, height);
        } else if (strncmp(line, "press", 5) == 0) {
            zwlr_virtual_pointer_v1_button(pointer, now(), 0x110, WL_POINTER_BUTTON_STATE_PRESSED);
        } else if (strncmp(line, "release", 7) == 0) {
            zwlr_virtual_pointer_v1_button(pointer, now(), 0x110, WL_POINTER_BUTTON_STATE_RELEASED);
        } else if (strncmp(line, "quit", 4) == 0) {
            break;
        } else {
            continue;
        }
        zwlr_virtual_pointer_v1_frame(pointer);
        wl_display_roundtrip(display);
        printf("done\n");
        fflush(stdout);
    }
    zwlr_virtual_pointer_v1_destroy(pointer);
    wl_display_roundtrip(display);
    return 0;
}
