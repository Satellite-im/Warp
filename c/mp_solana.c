#include <stdio.h>
#include <stdarg.h>
#include <stdlib.h>
#include <string.h>
#include "../extensions/warp-mp-solana/warp-mp-solana.h"
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

    if (!tesseract_set_file(tesseract, "datastore")) {
        printf("Error setting file\n");
        return -1;
    }

    if (!tesseract_set_autosave(tesseract)) {
        printf("Error setting autosave flag\n");
        return -1;
    }

    void *mp = multipass_mp_solana_new_with_devnet(NULL, tesseract);

    if (!mp) {
        printf("Unable to create multipass context\n");
        return -1;
    }

    //Doing this because the pointer is cloned internally for mp and no longer needed
    //TODO: Create a Arc<Mutex<_>> handle of Tesseract to pass into mp from ffi so tesseract can continue to be used

    tesseract_free(tesseract);

    if(!multipass_create_identity(mp, NULL, NULL)) {
        printf("Unable to create identity\n");
        return -1;
    }
    
    //TODO: Access to Identity struct

    struct Identity *id = multipass_get_own_identity(mp);

    if (!id) {
        printf("Unable to get identity\n");
        return -1;
    }

    char *username = multipass_identity_username(id);

    if (!username) {
        printf("Unable to get username");
        return -1;
    }

    uint16_t short_code = multipass_identity_short_id(id);

    printf("Identity Username: %s#%d\n", username, short_code);

    free(username);

    free(id);
    
    multipass_free(mp);
    return 0;
}