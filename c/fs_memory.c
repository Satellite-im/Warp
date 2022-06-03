#include <stdint.h>
#include <stdio.h>
#include <stdarg.h>
#include <stdlib.h>
#include <string.h>
#include "../warp/warp.h"
#include "../extensions/warp-fs-memory/warp-fs-memory.h"

int main() {
    ConstellationAdapter *memory = constellation_fs_memory_create_context();

    if (!memory) {
        printf("Cannot create contact for contellation\n");
        return -1;
    }

    const char *data = "Hello, World!";
    uint32_t data_size = strlen(data);
    FFIResult_c_void result_ignored_t = constellation_put_buffer(memory, "readme.txt", (const uint8_t*)data, data_size);
    if(result_ignored_t.error) {
        printf("Error uploading file\n");
        return -1;
    }


    FFIResult_FFIVec_u8 result_vu8_t = constellation_get_buffer(memory, "readme.txt");

    if (result_vu8_t.error) {
        return -1;
    }

    printf("%s\n", (const char*)result_vu8_t.data->ptr);

    // ConstellationDataType data_t = Json;

    // char *export = constellation_export(memory, data_t);

    // printf("%s\n", export);

    constellationadapter_free(memory);

    return 0;
}