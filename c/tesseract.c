#include <stdio.h>
#include <stdarg.h>
#include <stdlib.h>
#include <string.h>
#include "../warp/warp.h"


void print_error(FFIError* error) {
    printf("Error Type: %s\n", error->error_type);
    printf("Error Message: %s\n", error->error_message);
}

int main() {
    FFIResult_c_void result_ignored_t;
    FFIResult_c_char result_char_t;
    
    struct Tesseract *tesseract = tesseract_new();

    if (!tesseract) {
        printf("Error creating tesseract context\n");
        return -1;
    }
    
    result_ignored_t = tesseract_unlock(tesseract, "this is my super key");

    if (result_ignored_t.error) {
        printf("Error unlocking tesseract\n");
        print_error(result_ignored_t.error);
        return -1;
    }

    if (!tesseract_enable_key_check(tesseract)) {
        printf("Unable to enable key check\n");
        return -1;     
    }

    if (!tesseract_is_key_check_enabled(tesseract)) {
        printf("Key check is disabled\n");
        return -1;
    }

    result_ignored_t = tesseract_set(tesseract, "MYAPI", "MYVAL");

    if (result_ignored_t.error) {
        printf("Unable to insert key into tesseract\n");
        print_error(result_ignored_t.error);
        return -1;
    }

    if (!tesseract_exist(tesseract, "MYAPI")) {
        printf("Key does not exist within tesseract\n");
        return -1;
    }

    result_char_t = tesseract_retrieve(tesseract, "MYAPI");

    if (result_char_t.error) {
        printf("Unable to retrieve data from tesseract\n");
        print_error(result_char_t.error);
        return -1;
    }

    char *data = result_char_t.data;

    if (strcmp(data, "MYVAL") != 0) {
        printf("Data from tesseract is invalid\n");
        return -1;
    }

    result_ignored_t = tesseract_to_file(tesseract, "c_datastore");

    if (result_ignored_t.error) {
        printf("Unable to save to file\n");
        print_error(result_ignored_t.error);
        return -1;
    }

    free(data);
    // free(result_char_t.data);
    // free(result_char_t.error);
    tesseract_free(tesseract);
    
    return 0;
}