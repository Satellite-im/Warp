#include <stdint.h>
#include <stdio.h>
#include <stdarg.h>
#include <stdlib.h>
#include <string.h>
#include "../warp/warp.h"
#include "../extensions/warp-fs-storj/warp-fs-storj.h"
#include "../extensions/warp-pd-flatfile/warp-pd-flatfile.h"

bool import_from_cache(struct PocketDimensionAdapter* pd, struct ConstellationAdapter* constellation) {
    DataType data_t = DataExport;

    struct FFIArray_Data *object = pocket_dimension_get_data(pd, data_t, NULL);
    if (!object) {
        return false;
    }

    int data_length = ffiarray_data_length(object);

    if (data_length==0) {
        return false;
    } 

    const struct Data *data = ffiarray_data_get(object, data_length - 1);

    if (!data) {
        return false;
    }

    char* data_j = data_payload(data);

    if (!data_j) {
        return false;
    }

    ConstellationDataType constellation_data_t = Json;

    if (!constellation_import(constellation, constellation_data_t, data_j)) {
        return false;
    }

    free((void *)data);
    free(object);
    free(data_j);

    return true;
}

bool export_to_cache(struct PocketDimensionAdapter* pd, struct ConstellationAdapter* constellation) {
    DataType data_t = DataExport;
    ConstellationDataType constellation_data_t = Json;
    char* data_j = constellation_export(constellation, constellation_data_t);

    if (!data_j) {
        return false;
    }

    struct Data* data = data_new(data_t, data_j);

    if (!data) {
        return false;
    }

    if(!pocket_dimension_add_data(pd, data_t, data)) {
        return false;
    }

    free(data_j);
    free(data);

    return true;
}

int main() {
    //Specific to linux path. Change the directory path if using on windows
    struct PocketDimensionAdapter *pd = pocket_dimension_flatfile_new("/tmp/c-cache", "data-cache");

    if (!pd) {
        printf("Cannot create context for pd\n");
        return -1;
    }

    //Change these 2 arguments for your access and secret key from storj
    //See https://raw.githack.com/Satellite-im/warp-docs/master/index.html#/extensions/constellation/storj to learn how
    //to get access and secret key
    struct ConstellationAdapter *fs_storj = constellation_fs_storj_new(pd, "<ACCESS_KEY>", "<SECRET_KEY>"); 
    if (!fs_storj) {
        printf("Cannot create context for contellation\n");
        return -1;
    }

    if (!import_from_cache(pd, fs_storj)) {
        printf("WARNING: Failed to import from cache\n");
    }

    const char *data = "Hello, World!";
    uint32_t data_size = strlen(data);

    if(!constellation_put_buffer(fs_storj, "readme.txt", (const uint8_t*)data, data_size)) {
        printf("Error uploading file\n");
    }


    uint8_t* return_data = constellation_get_buffer(fs_storj, "readme.txt");

    if (!return_data) {
        printf("Error downloading file\n");
        return -1;
    }

    printf("%s\n", (const char*)data);

    if(!export_to_cache(pd, fs_storj)) {
        printf("WARNING: Failed to import from cache\n");
    }
    
    free(return_data);
    constellationadapter_free(fs_storj);
    pocket_dimension_free(pd);
    return 0;
}