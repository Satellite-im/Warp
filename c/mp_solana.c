#include <stdio.h>
#include <stdarg.h>
#include <stdlib.h>
#include <string.h>
#include "../warp/warp.h"
#include "../extensions/warp-mp-solana/warp-mp-solana.h"


MultiPassAdapter *new_account(const char* file) {
    struct Tesseract *tesseract;
    
    if (!(tesseract = tesseract_from_file(file))) {
        tesseract = tesseract_new();
    }

    if (!tesseract) {
        return NULL;
    }

    if (!tesseract_unlock(tesseract, "this is my super key")) {
        return NULL;
    }

    if (!tesseract_set_file(tesseract, file)) {
        printf("Unable to set file for tesseract with \"%s\". Skipped...\n", file);
    }

    if (!tesseract_set_autosave(tesseract)) {
        printf("Unable to enable autosave for tesseract. Skipped...\n");
    }

    if (!tesseract_autosave_enabled(tesseract)) {
        printf("Autosave is disabled\n");
    }

    struct MultiPassAdapter *mp = multipass_mp_solana_new_with_devnet(NULL, tesseract);

    if (!mp) {
        return NULL;
    }

    tesseract_free(tesseract);

    if(!multipass_create_identity(mp, NULL, NULL)) {
        if (!multipass_get_own_identity(mp)) {
            return NULL;
        }
    }

    return (MultiPassAdapter *)mp;
}

bool print_identity(struct Identity *id) {

    if (!id) {
        printf("Unable to get identity\n");
        return false;
    }

    char *username = multipass_identity_username(id);

    if (!username) {
        printf("Unable to get username");
        return false;
    }

    uint16_t short_code = multipass_identity_short_id(id);

    printf("Account Username: %s#%d\n", username, short_code);

    free(username);

    return true;
}

int main() {
    
    MultiPassAdapter *account_a = new_account("account_a");

    if(!account_a) {
        printf("Account A is NULL\n");
        return -1;
    }

    MultiPassAdapter *account_b = new_account("account_b");
    if(!account_b) {
        printf("Account B is NULL\n");
        return -1;
    }

    struct Identity *ident_a = multipass_get_own_identity(account_a);
    struct Identity *ident_b = multipass_get_own_identity(account_b);

    print_identity(ident_a);

    print_identity(ident_b);

    //Assuming that the identity isnt null
    const struct PublicKey *acct_a_key = multipass_identity_public_key(ident_a);
    const struct PublicKey *acct_b_key = multipass_identity_public_key(ident_b);

    if(!multipass_has_friend(account_a, acct_b_key)) {
        if(!multipass_send_request(account_a, acct_b_key)) {
            printf("Unable to send friend request\n");
            goto drop;
        }

        if(!multipass_accept_request(account_b, acct_a_key)) {
            printf("Unable to accept friend request\n");
            goto drop;
        }
    }

    const struct FFIArray_Identity *friends = multipass_list_friends(account_a);

    int len = ffiarray_identity_length(friends);

    for(int i = 0; i<len; i++) {
        const struct Identity *friend = ffiarray_identity_get(friends, i);
        char *friend_name = multipass_identity_username((struct Identity *)friend);
        if (!friend_name) {
            printf("Unable to get username in iter %d\n", i);
            continue;
        }

        uint16_t sc = multipass_identity_short_id((struct Identity *)friend);

        printf("Friend Identity Username: %s#%d\n", friend_name, sc);
        free(friend_name);
    }

    drop:
    multipass_identity_free(ident_a);
    multipass_identity_free(ident_b);
    multipass_free(account_a);
    multipass_free(account_b);
    return 0;
}