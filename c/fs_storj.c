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

    FFIResult_FFIVec_Data result_object_t = pocket_dimension_get_data(pd, data_t, NULL);
    if (result_object_t.error) {
        return false;
    }

    FFIVec_Data *object = result_object_t.data;

    if (object->len==0) {
        return false;
    } 

    struct Data *data = object->ptr[object->len - 1];

    if (!data) {
        return false;
    }

    FFIResult_String result_payload_t = data_payload(data);

    if (result_payload_t.error) {
        return false;
    }

    const char* data_j = result_payload_t.data;

    ConstellationDataType constellation_data_t = Json;


    FFIResult_c_void result_ignore = constellation_import(constellation, constellation_data_t, data_j);
    if (result_ignore.error) {
        return false;
    }

    free((void *)data);
    free(object);
    free((void *)data_j);

    return true;
}

bool export_to_cache(struct PocketDimensionAdapter* pd, struct ConstellationAdapter* constellation) {
    DataType data_t = DataExport;
    ConstellationDataType constellation_data_t = Json;
    FFIResult_String result_char_t = constellation_export(constellation, constellation_data_t);

    if (result_char_t.error) {
        return false;
    }

    FFIResult_Data result_data = data_new(data_t, result_char_t.data);

    if (result_data.error) {
        return false;
    }
    FFIResult_c_void result_ignored = pocket_dimension_add_data(pd, data_t, result_data.data);
    if(result_ignored.error) {
        return false;
    }

    free(result_char_t.data);
    free(result_data.data);

    return true;
}

int main() {
    //Specific to linux path. Change the directory path if using on windows
    FFIResult_PocketDimensionAdapter result_pd = pocket_dimension_flatfile_new("/tmp/c-cache", "data-cache");

    if (result_pd.error) {
        printf("Cannot create context for pd\n");
        return -1;
    }

    PocketDimensionAdapter *pd = result_pd.data;

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

    FFIResult_c_void result_ignore_t = constellation_put_buffer(fs_storj, "readme.txt", (const uint8_t*)data, data_size);

    if(result_ignore_t.error) {
        printf("Error uploading file\n");
    }


    FFIResult_FFIVec_u8 result_data_t = constellation_get_buffer(fs_storj, "readme.txt");

    if (result_data_t.error) {
        printf("Error downloading file\n");
        return -1;
    }

    printf("%s\n", (const char*)result_data_t.data->ptr);

    if(!export_to_cache(pd, fs_storj)) {
        printf("WARNING: Failed to import from cache\n");
    }
    
    free(result_data_t.data);
    constellationadapter_free(fs_storj);
    pocketdimensionadapter_free(pd);
    return 0;
}