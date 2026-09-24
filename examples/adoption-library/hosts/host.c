/* A C host: an existing native application adopting a Rust Native model
   through the generated header (`rustnative bindgen counter.ril --lang c`).
   Exits 0 when every expectation holds. */
#include <stdio.h>
#include <string.h>
#include "counter.h"

static uint32_t last_changed = 0;
static int changes = 0;

static void on_changed(void* context, uint32_t count) {
    (void)context;
    last_changed = count;
    changes++;
}

#define EXPECT(condition)                                                  \
    do {                                                                   \
        if (!(condition)) {                                                \
            fprintf(stderr, "failed: %s (%s)\n", #condition,               \
                    counter_last_error());                                  \
            return 1;                                                      \
        }                                                                  \
    } while (0)

int main(void) {
    counter_Counter counter = 0;
    uint32_t count = 0;
    char* label = NULL;

    EXPECT(counter_Counter_new(40, &counter) == COUNTER_OK);
    EXPECT(counter_Counter_on_changed(counter, on_changed, NULL) == COUNTER_OK);
    EXPECT(counter_Counter_increment(counter, &count) == COUNTER_OK);
    EXPECT(counter_Counter_increment(counter, &count) == COUNTER_OK);
    EXPECT(count == 42);
    EXPECT(changes == 2 && last_changed == 42);

    EXPECT(counter_Counter_rename(counter, "Tally") == COUNTER_OK);
    EXPECT(counter_Counter_label(counter, &label) == COUNTER_OK);
    EXPECT(strcmp(label, "Tally: 42") == 0);
    counter_string_free(label);

    /* Failures are status codes with a message, never a crash. */
    EXPECT(counter_Counter_rename(counter, NULL) == COUNTER_INVALID_ARGUMENT);
    EXPECT(strstr(counter_last_error(), "null") != NULL);

    EXPECT(counter_Counter_free(counter) == COUNTER_OK);
    EXPECT(counter_Counter_increment(counter, &count) == COUNTER_INVALID_HANDLE);
    printf("ok\n");
    return 0;
}
