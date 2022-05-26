#include <stdio.h>
#include <stdarg.h>
#include <stdlib.h>
#include <string.h>
#include "../warp/warp.h"
#include "../extensions/warp-pd-flatfile/warp-pd-flatfile.h"


int main() {
    
    struct PocketDimensionAdapter *pd = pocket_dimension_flatfile_new("/tmp/c-cache", "data-cache");

    if (!pd) {
        printf("PD is null\n");
        return -1;
    }
    
    DataType data_t = FileSystem; 

    //Based on `DimensionData::from_buffer` but in json format
    struct Data *data_set1 = data_new(data_t, "{\"name\":\"data_1\",\"buffer\":[100,97,116,97]}");
    struct Data *data_set2 = data_new(data_t, "{\"name\":\"data_2\",\"buffer\":[100,97,116,97]}");

    if (!pocket_dimension_add_data(pd, data_t, data_set1)) {
        printf("Unable to add object to pd\n");
        return -1;
    }

    if (!pocket_dimension_add_data(pd, data_t, data_set2)) {
        printf("Unable to add object to pd\n");
        return -1;
    }

    //Uses json to for the query structure 
    struct QueryBuilder* query = querybuilder_import("{\"where\":[],\"comparator\":[{\"eq\":[\"name\",\"data_1\"]}],\"limit\":1}");

    if (!query) {
        printf("invalid query\n");
        return -1; 
    } 
 
    struct FFIArray_Data* internal = pocket_dimension_get_data(pd, data_t, query);

    int length = ffiarray_data_length(internal);

    for(int i = 0; i<length; i++) {
        const struct Data *dat = ffiarray_data_get(internal, i);

        if (!dat) {
            printf("Error: data is null\n");
            return -1;
        }

        const char *id = data_id(dat);
 
        printf("%s\n", id);

        const char *payload_str = data_payload(dat);

        if (!payload_str) { 
            return -1;
        }

        printf("%s\n", payload_str);
    }


    return 0;
}

