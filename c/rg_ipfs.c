// This example is just sending messages internally and not an actual chat interface like the rust example. 
#include <stdio.h>
#include <unistd.h>
#include <stdarg.h>
#include <stdlib.h>
#include <string.h>
#include "../warp/warp.h"
#include "../extensions/warp-mp-ipfs/warp-mp-ipfs.h"
#include "../extensions/warp-rg-ipfs/warp-rg-ipfs.h"

void print_error(FFIError* error) {
    printf("Error Type: %s\n", error->error_type);
    printf("Error Message: %s\n", error->error_message);
    ffierror_free(error);
}

MultiPassAdapter *new_account(const char* pass) {
    
    Tesseract *tesseract = tesseract_new();;

    FFIResult_Null result_unlock_t = tesseract_unlock(tesseract, pass);
    if (result_unlock_t.error) {
        print_error(result_unlock_t.error);
        return NULL;
    }

    MpIpfsConfig *config = mp_ipfs_config_testing();


    FFIResult_MultiPassAdapter result_mp = multipass_mp_ipfs_temporary(NULL, tesseract, config);

    if (result_mp.error) {
        print_error(result_mp.error);
        return NULL;
    }

    tesseract_free(tesseract);
    FFIResult_DID result_ignore_t = multipass_create_identity(result_mp.data, NULL, NULL);
    if (result_ignore_t.error) {
        printf("Error creating identity\n");
        print_error(result_ignore_t.error);
        return NULL;
    }
    free(config);
    return result_mp.data;
}

RayGunAdapter *new_chat(const MultiPassAdapter* mp) {
    RgIpfsConfig *config = rg_ipfs_config_testing();
    FFIResult_RayGunAdapter result_rg_t = warp_rg_ipfs_temporary_new(mp, NULL, config);
    if (result_rg_t.error) {
        print_error(result_rg_t.error);
        return NULL;
    }
    free(config);
    return result_rg_t.data;
}

char *get_username(const MultiPassAdapter *account, const SenderId* id) {
    if (!id) {
        return "Null User";
    }

    DID *key = sender_id_get_did_key(id);

    Identifier *ident = multipass_identifier_did_key(key);

    if (!ident) {
        return "Null User";
    }

    FFIResult_Identity result_identity_t = multipass_get_identity(account, ident);

    if(result_identity_t.error) {
        print_error(result_identity_t.error);
        return "Null User";
    }
    return multipass_identity_username(result_identity_t.data);
}

void print_messages(const MultiPassAdapter *account, const FFIVec_Message *messages) {

    for (uintptr_t i = 0; i<messages->len; i++) {
        Message *message = messages->ptr[i];
        if (!message) {
            printf("Message is null\n");
            return;
        }

        SenderId* sender = message_sender_id(message);
        char *username = get_username(account, sender);

        FFIVec_String *messages_vec = message_lines(message);
        
        if (messages_vec->len >= 1) {
            for (uintptr_t x = 0; x < messages_vec->len; x++) {
                char *line = messages_vec->ptr[x];
                if (line)
                    printf("%s: %s\n", username, line);
            }
        }
    }

}

int main() {
    MultiPassAdapter *account_a = new_account("c_datastore_a");
    MultiPassAdapter *account_b = new_account("c_datastore_b");

    if (!account_a || !account_b) {
        printf("Error creating account\n");
        return -1;
    }

    FFIResult_Identity result_ident_b = multipass_get_own_identity(account_b);

    if (result_ident_b.error) {
        print_error(result_ident_b.error);
        return -1;
    }

    Identity *ident_b = result_ident_b.data;

    RayGunAdapter* chatter_a = new_chat(account_a);
    RayGunAdapter* chatter_b = new_chat(account_b);

    if (!chatter_a || !chatter_b) {
        printf("Error creating account\n");
        return -1;
    }

    sleep(1);
    DID *ident_b_did = multipass_identity_did_key(ident_b);
    FFIResult_String result_convo = raygun_create_conversation(chatter_a, ident_b_did);
    if (result_convo.error) {
        print_error(result_convo.error);
        return -1;
    }

    char *conversation_id = result_convo.data;
    sleep(1);
    //Messages are sent to rust via ffi as a array pointer. We would 
    const char *chat_a_message[] = {
        "Hello, World!!!", 
        "How are you??", 
        "Has your day been good???", 
        "Mine is great", 
        "You there????",
        "Just tired from dealing with C :D", 
        "Rust rules!!!"
    };

    int message_lines_length = sizeof(chat_a_message) / sizeof(chat_a_message[0]);

    FFIResult_Null result_chat_t = raygun_send(chatter_a, conversation_id, NULL, chat_a_message, message_lines_length);

    if (result_chat_t.error) {
        print_error(result_chat_t.error);
        return -1;
    }
    // Because its async internally, we want to make sure that chatter_b to receive it before attempting to . 
    sleep(1);

    FFIResult_FFIVec_Message result_messages_b_t = raygun_get_messages(chatter_b, conversation_id);

    if (result_messages_b_t.error) {
        print_error(result_messages_b_t.error);
        return -1;
    }

    printf("Chatter B messages\n");
    print_messages(account_b, result_messages_b_t.data);
    printf("\n");

    //Messages are sent to rust via ffi as a array pointer. We would 
    const char *chat_b_message[] = {
        "Hello from Chatter A :D",
        "I've grown tired of C",
        "Rust is life",
        "Sooooooooooo tired",
        "Dreamed of being within a dream and waking up from that dream while in a dream :D"
    };

    int message_b_lines_length = sizeof(chat_b_message) / sizeof(chat_b_message[0]);
    FFIResult_Null result_chat_b_t = raygun_send(chatter_b, conversation_id, NULL, chat_b_message, message_b_lines_length);

    if (result_chat_b_t.error) {
        print_error(result_chat_b_t.error);
        return -1;
    }

    // Ditto; We may implement a small "wait" in rust to make sure the peers are synced up in some manner.  
    sleep(1);

    //Note: Because chatter a already had messages stored that was previously sent
    FFIResult_FFIVec_Message result_messages_t = raygun_get_messages(chatter_a, conversation_id);

    if (result_messages_t.error) {
        print_error(result_messages_t.error);
        return -1;
    }

    printf("Chatter A messages\n");
    print_messages(account_a, result_messages_t.data);

    //     const char *message[] = {"Hello, World!!!"};

    // FFIResult_c_void result_chat_t = raygun_send(chatter_a, conversation_id, NULL, message, 1);
    return 0;
}

