#include <stdio.h>
#include <stdarg.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <pthread.h>

#include "../warp/warp.h"
#include "../extensions/warp-mp-ipfs/warp-mp-ipfs.h"

void print_error(FFIError *error)
{
    printf("Error Type: %s\n", error->error_type);
    printf("Error Message: %s\n", error->error_message);
}

typedef struct EventStream
{
    DID *did;
    MultiPassEventStream *stream;
} EventStream;

void *stream_thread(void *arg)
{
    EventStream *event_stream = arg;
    MultiPassEventStream *stream = event_stream->stream;
    char *did_key = did_to_string(event_stream->did);
    while (1)
    {
        FFIResult_String result_event = multipass_stream_next(stream);
        if (result_event.error)
        {
            print_error(result_event.error);
            break;
        }
        printf("[%s] -> Event: %s\n", did_key, result_event.data);
        free(result_event.data);
    }
    multipasseventstream_free(stream);
    free(event_stream);
    return NULL;
}

MultiPassAdapter *new_account()
{

    MpIpfsConfig *config = mp_ipfs_config_development();

    struct Tesseract *tesseract = tesseract_new();

    FFIResult_Null result_unlock_t = tesseract_unlock(tesseract, "this is my super key");
    if (result_unlock_t.error)
    {
        print_error(result_unlock_t.error);
        return NULL;
    }

    FFIResult_MultiPassAdapter result_mp = multipass_mp_ipfs_temporary(NULL, tesseract, config);

    if (result_mp.error)
    {
        print_error(result_mp.error);
        return NULL;
    }

    MultiPassAdapter *mp = result_mp.data;

    tesseract_free(tesseract);
    FFIResult_DID result_t = multipass_create_identity(mp, NULL, NULL);
    if (result_t.error)
    {
        print_error(result_t.error);
        return NULL;
    }

    FFIResult_MultiPassEventStream result_stream = multipass_subscribe(mp);
    if (result_stream.error)
    {
        print_error(result_stream.error);
        return NULL;
    }

    EventStream event_stream = {.did = result_t.data, .stream = result_stream.data};
    pthread_t acct_thr;
    pthread_create(&acct_thr, NULL, stream_thread, (void *)&event_stream);

    sleep(1);
    return mp;
}

bool print_identity(struct Identity *id)
{

    if (!id)
    {
        printf("Unable to get identity\n");
        return false;
    }

    char *username = multipass_identity_username(id);

    if (!username)
    {
        printf("Unable to get username");
        return false;
    }

    char *short_code = multipass_identity_short_id(id);

    printf("Account Username: %s#%s\n", username, short_code);

    free(username);
    free(short_code);
    return true;
}

int main()
{

    MultiPassAdapter *account_a = new_account();

    if (!account_a)
    {
        printf("Account A is NULL\n");
        return -1;
    }

    MultiPassAdapter *account_b = new_account();
    if (!account_b)
    {
        printf("Account B is NULL\n");
        return -1;
    }

    FFIResult_Identity result_ident_a = multipass_get_own_identity(account_a);
    FFIResult_Identity result_ident_b = multipass_get_own_identity(account_b);

    if (result_ident_a.error)
    {
        print_error(result_ident_a.error);
        return -1;
    }

    if (result_ident_b.error)
    {
        print_error(result_ident_b.error);
        return -1;
    }

    Identity *ident_a = result_ident_a.data;
    Identity *ident_b = result_ident_b.data;
    print_identity(ident_a);
    print_identity(ident_b);

    // Assuming that the identity isnt null
    struct DID *acct_a_key = multipass_identity_did_key(ident_a);
    struct DID *acct_b_key = multipass_identity_did_key(ident_b);

    FFIResult_Null result_ignore0_t = multipass_send_request(account_a, acct_b_key);
    if (result_ignore0_t.error)
    {
        printf("Unable to send friend request\n");
        print_error(result_ignore0_t.error);
        goto drop;
    }
    sleep(1);

    FFIResult_Null result_ignore1_t = multipass_accept_request(account_b, acct_a_key);
    if (result_ignore1_t.error)
    {
        printf("Unable to accept friend request\n");
        print_error(result_ignore1_t.error);
        goto drop;
    }

    sleep(1);
    struct FFIResult_FFIVec_DID result_friends = multipass_list_friends(account_a);

    if (result_friends.error)
    {
        print_error(result_friends.error);
        goto drop;
    }

    FFIVec_DID *friends = result_friends.data;

    for (uintptr_t i = 0; i < friends->len; i++)
    {
        DID *friend = friends->ptr[i];

        FFIResult_FFIVec_Identity result_identity = multipass_get_identity(account_a, multipass_identifier_did_key(friend));

        Identity *identity = result_identity.data->ptr[0];

        char *friend_name = multipass_identity_username(identity);
        if (!friend_name)
        {
            printf("Unable to get username in iter %lu\n", i);
            continue;
        }

        char *sc = multipass_identity_short_id(identity);

        printf("Friend Identity Username: %s#%s\n", friend_name, sc);
        free(identity);
        free(sc);
        free(friend_name);
    }

drop:
    identity_free(ident_a);
    identity_free(ident_b);
    multipassadapter_free(account_a);
    multipassadapter_free(account_b);
    return 0;
}