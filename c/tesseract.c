#include <stdio.h>
#include <stdarg.h>
#include <stdlib.h>
#include <string.h>
#include "../warp/warp.h"


int main() {
    
    struct Tesseract *tesseract = tesseract_new();

    if (!tesseract) {
        printf("Error creating tesseract context\n");
        return -1;
    }

    if (!tesseract_unlock(tesseract, "this is my super key")) {
        printf("Error unlocking tesseract\n");
        return -1;
    }


    if (!tesseract_set(tesseract, "MYAPI", "MYVAL")) {
        printf("Unable to insert key into tesseract\n");
        return -1;
    }

    if (!tesseract_exist(tesseract, "MYAPI")) {
        printf("Key does not exist within tesseract\n");
        return -1;
    }

    char *data = tesseract_retrieve(tesseract, "MYAPI");

    if (!data) {
        printf("Unable to retrieve data from tesseract\n");
        return -1;
    }

    if (strcmp(data, "MYVAL") != 0) {
        printf("Data from tesseract is invalid\n");
        return -1;
    }

    if (!tesseract_to_file(tesseract, "c_datastore")) {
        printf("Unable to save to file\n");
        return -1;
    }

    free(data);

    tesseract_free(tesseract);
    
    return 0;
}