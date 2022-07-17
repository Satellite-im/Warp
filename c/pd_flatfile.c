#include <stdio.h>
#include <stdarg.h>
#include <stdlib.h>
#include <string.h>
#include "../warp/warp.h"
#include "../extensions/warp-pd-flatfile/warp-pd-flatfile.h"

void print_error(FFIError* error) {
    printf("Error Type: %s\n", error->error_type);
    printf("Error Message: %s\n", error->error_message);
    ffierror_free(error);
}

int main() {
    
    FFIResult_PocketDimensionAdapter result_pd_t = pocket_dimension_flatfile_new("/tmp/c-cache", "data-cache");

    if (result_pd_t.error) {
        printf("Error with pd\n");
        print_error(result_pd_t.error);
        return -1;
    }
    
    PocketDimensionAdapter *pd = result_pd_t.data;

    DataType data_t = FileSystem; 

    //Based on `DimensionData::from_buffer` but in json format
    FFIResult_Data result_data_set1 = data_new(data_t, "{\"name\":\"data_1\",\"internal\":[100,97,116,97]}");
    FFIResult_Data result_data_set2 = data_new(data_t, "{\"name\":\"data_2\",\"internal\":[100,97,116,97]}");

    if (result_data_set1.error) {
        print_error(result_data_set1.error);
        return -1;
    }

    if (result_data_set2.error) {
        print_error(result_data_set2.error);
        return -1;
    }

    FFIResult_Null result_ignored_t = pocket_dimension_add_data(pd, data_t, result_data_set1.data);

    if (result_ignored_t.error) {
        printf("Unable to add object to pd\n");
        print_error(result_ignored_t.error);
        return -1;
    }

    result_ignored_t = pocket_dimension_add_data(pd, data_t, result_data_set2.data);
    if (result_ignored_t.error) {
        printf("Unable to add object to pd\n");
        print_error(result_ignored_t.error);
        return -1;
    }

    //Uses json to for the query structure 
    FFIResult_QueryBuilder result_query_t = querybuilder_import("{\"where\":[],\"comparator\":[{\"eq\":[\"name\",\"data_1\"]}],\"limit\":1}");

    if (result_query_t.error) {
        printf("invalid query\n");
        print_error(result_query_t.error);
        return -1; 
    } 
    
    struct FFIResult_FFIVec_Data result_array_t = pocket_dimension_get_data(pd, data_t, result_query_t.data);

    //Note: skipping check for now

    int length = ffiarray_data_length(result_array_t.data);

    for(int i = 0; i<result_array_t.data->len; i++) {
        Data *dat = result_array_t.data->ptr[i];

        if (!dat) {
            printf("Error: data is null\n");
            return -1;
        }

        char *id = data_id(dat);
 
        printf("%s\n", id);

        FFIResult_String result_payload_t = data_payload(dat);

        if (result_payload_t.error) {
            print_error(result_payload_t.error); 
            return -1;
        }

        printf("%s\n", result_payload_t.data);
        free(result_payload_t.data);
        free(id);
    }

    //TODO: Free more pointers
    free(result_data_set2.data);
    free(result_data_set1.data);
    ffivec_data_free(result_array_t.data);
    querybuilder_free(result_query_t.data);
    return 0;
}

