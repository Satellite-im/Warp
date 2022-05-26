#include <stdio.h>
#include <stdarg.h>
#include <stdlib.h>
#include <string.h>
#include "../extensions/warp-mp-solana/warp-mp-solana.h"
#include "../warp/warp.h"


MultiPassAdapter *new_account() {
    struct Tesseract *tesseract = tesseract_new();

    if (!tesseract) {
        return NULL;
    }

    if (!tesseract_unlock(tesseract, "this is my super key")) {
        return NULL;
    }

    void *mp = multipass_mp_solana_new_with_devnet(NULL, tesseract);

    if (!mp) {
        return NULL;
    }

    tesseract_free(tesseract);

    if(!multipass_create_identity(mp, NULL, NULL)) {
        return NULL;
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
    
    MultiPassAdapter *account_a = new_account();

    if(!account_a) {
        printf("Account A is NULL\n");
        return -1;
    }

    MultiPassAdapter *account_b = new_account();
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

    if(!multipass_send_request(account_a, acct_b_key)) {
        printf("Unable to send friend request\n");
        goto drop;
    }

    // List account_b request and check to see if account_a sent a request
    // const struct FriendRequest *requests[] = {multipass_list_incoming_request(account_b)};

    // int request_len = sizeof(requests) / sizeof(requests[0]);

    // if (request_len == 0) {
    //     printf("No incoming friend request\n");
    //     goto drop;
    // }

    // bool match = false;

    // for(int i = 0; i<request_len; i++) {
    //     const struct FriendRequest *request = requests[i];
    //     struct PublicKey*from_key = multipass_friend_request_from((struct FriendRequest *)request);

    //     if (!from_key) {
    //         printf("Public key is null at index[%d]\n", i);
    //         free((void *)request);
    //         continue;
    //     }

    //     if(from_key == acct_a_key) {
    //         printf("Request is not from account a\n");
    //         free((void *)from_key);
    //         free((void *)request);
    //         continue;
    //     }

    //     match = true;
    //     free((void *)from_key);
    //     free((void *)request);
    // }

    // if (!match) {
    //     printf("Keys do not match\n");
    //     goto drop;
    // }

    if(!multipass_accept_request(account_b, acct_a_key)) {
        printf("Unable to accept friend request\n");
        goto drop;
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