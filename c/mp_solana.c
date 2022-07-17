#include <stdio.h>
#include <stdarg.h>
#include <stdlib.h>
#include <string.h>
#include "../warp/warp.h"
#include "../extensions/warp-mp-solana/warp-mp-solana.h"


void print_error(FFIError* error) {
    printf("Error Type: %s\n", error->error_type);
    printf("Error Message: %s\n", error->error_message);
}

MultiPassAdapter *new_account(const char* file) {
    
    struct Tesseract *tesseract = NULL;
    FFIResult_Tesseract result_tesseract_t = tesseract_from_file(file);

    if (result_tesseract_t.error) {
        print_error(result_tesseract_t.error);
        tesseract = tesseract_new();
    } else if (result_tesseract_t.data) {
        tesseract = result_tesseract_t.data;
    }

    FFIResult_Null result_unlock_t = tesseract_unlock(tesseract, "this is my super key");
    if (result_unlock_t.error) {
        print_error(result_unlock_t.error);
        return NULL;
    }

    tesseract_set_file(tesseract, file);

    tesseract_set_autosave(tesseract);

    if (!tesseract_autosave_enabled(tesseract)) {
        printf("Autosave is disabled\n");
    }

    struct MultiPassAdapter *mp = multipass_mp_solana_new_with_devnet(NULL, tesseract);

    if (!mp) {
        printf("Adapter is null\n");
        return NULL;
    }

    tesseract_free(tesseract);
    FFIResult_PublicKey result_ignore_t = multipass_create_identity(mp, NULL, NULL);
    if (result_ignore_t.error) {
        if(strcmp(result_ignore_t.error->error_type, "Any") && strcmp(result_ignore_t.error->error_message, "Account already exist")) {
            FFIResult_Identity result_identity_t = multipass_get_own_identity(mp);
            if (result_identity_t.error) {
                print_error(result_identity_t.error);
                return NULL;
            }
        } else {
            printf("Error?\n");
            print_error(result_ignore_t.error);
            return NULL;
        }
    }

    return mp;
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

    FFIResult_Identity result_ident_a = multipass_get_own_identity(account_a);
    FFIResult_Identity result_ident_b = multipass_get_own_identity(account_b);

    if (result_ident_a.error) {
        print_error(result_ident_a.error);
        return -1;
    }

    if (result_ident_b.error) {
        print_error(result_ident_b.error);
        return -1;
    }

    Identity *ident_a = result_ident_a.data;
    Identity *ident_b = result_ident_b.data;
    print_identity(ident_a);
    print_identity(ident_b);

    //Assuming that the identity isnt null
    struct PublicKey *acct_a_key = multipass_identity_public_key(ident_a);
    struct PublicKey *acct_b_key = multipass_identity_public_key(ident_b);

    FFIResult_Null result_void = multipass_has_friend(account_a, acct_b_key);

    if(!result_void.error) {
        FFIResult_Null result_ignore0_t = multipass_send_request(account_a, acct_b_key);
        if(result_ignore0_t.error) {
            printf("Unable to send friend request\n");
            print_error(result_ignore0_t.error);
            goto drop;
        }

        FFIResult_Null result_ignore1_t = multipass_accept_request(account_b, acct_a_key);
        if(result_ignore1_t.error) {
            printf("Unable to accept friend request\n");
            print_error(result_ignore1_t.error);
            goto drop;
        }
    }

    struct FFIResult_FFIVec_PublicKey result_friends = multipass_list_friends(account_a);

    if(result_friends.error) {
        print_error(result_friends.error);
        goto drop;
    }

    FFIVec_PublicKey *friends = result_friends.data;

    for(int i = 0; i<friends->len; i++) {
        PublicKey *friend = friends->ptr[i];
        
        FFIResult_Identity result_identity = multipass_get_identity(account_a, multipass_identifier_public_key(friend));

        char *friend_name = multipass_identity_username(result_identity.data);
        if (!friend_name) {
            printf("Unable to get username in iter %d\n", i);
            continue;
        }

        uint16_t sc = multipass_identity_short_id(result_identity.data);

        printf("Friend Identity Username: %s#%d\n", friend_name, sc);
        free(friend_name);
    }

    drop:
    identity_free(ident_a);
    identity_free(ident_b);
    multipassadapter_free(account_a);
    multipassadapter_free(account_b);
    return 0;
}