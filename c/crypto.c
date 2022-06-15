#include <stdint.h>
#include <stdio.h>
#include <stdarg.h>
#include <stdlib.h>
#include <string.h>
#include "../warp/warp.h"

void print_error(FFIError* error) {
    printf("Error Type: %s\n", error->error_type);
    printf("Error Message: %s\n", error->error_message);
    ffierror_free(error);
}

int main() {
    char *plaintext = "Hello, World!";
    int plaintext_len = strlen(plaintext);
    Cipher *cipher = cipher_new();
    CipherType cipher_type = Aes256Gcm;
    FFIResult_FFIVec_u8 result_t;
    result_t = cipher_encrypt(cipher, cipher_type, (uint8_t *) plaintext, plaintext_len);

    if (result_t.error) {
        print_error(result_t.error);
        return -1;
    }

    FFIVec_u8* encrypt = result_t.data;

    result_t = cipher_decrypt(cipher, cipher_type, encrypt->ptr, encrypt->len);

    if (result_t.error) {
        print_error(result_t.error);
        return -1;
    }

    FFIVec_u8* decrypt = result_t.data;
    printf("%s\n", (char *)decrypt->ptr);

    return 0;
}