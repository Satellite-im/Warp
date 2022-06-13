#include <stdint.h>
#include <stdio.h>
#include <stdarg.h>
#include <stdlib.h>
#include <string.h>
#include "../warp/warp.h"

int main() {
    char *plaintext = "Hello, World!";
    int plaintext_len = strlen(plaintext);

    FFIVec_u8 *encrypt = crypto_aes256gcm_self_encrypt((uint8_t *) plaintext, plaintext_len);

    if (!encrypt) {
        printf("Error encrypting\n");
        return -1;
    }

    FFIVec_u8 *decrypt = crypto_aes256gcm_self_decrypt(encrypt->ptr, encrypt->len);

    if (!decrypt) {
        printf("Error decrypting\n");
        return -1;
    }

    printf("%s\n", (char *)decrypt->ptr);

    return 0;
}