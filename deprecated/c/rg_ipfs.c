// This example is just sending messages internally and not an actual chat interface like the rust example.
#include <stdio.h>
#include <unistd.h>
#include <stdarg.h>
#include <stdlib.h>
#include <string.h>
#include <threads.h>

#include "../warp/warp.h"
#include "../extensions/warp-mp-ipfs/warp-mp-ipfs.h"
#include "../extensions/warp-rg-ipfs/warp-rg-ipfs.h"

void print_error(FFIError *error)
{
    printf("Error Type: %s\n", error->error_type);
    printf("Error Message: %s\n", error->error_message);
    ffierror_free(error);
}

typedef struct RGEventStream
{
    DID *did;
    RayGunEventStream *stream;
} RGEventStream;

typedef struct MsgEventStream
{
    DID *did;
    char *conversation_id;
    MessageEventStream *stream;
} MsgEventStream;

void *raygun_stream_thread(void *arg)
{
    RGEventStream *event_stream = arg;
    RayGunEventStream *stream = event_stream->stream;
    char *did_key = did_to_string(event_stream->did);
    while (1)
    {
        FFIResult_String result_event = raygun_stream_next(stream);
        if (result_event.error)
        {
            print_error(result_event.error);
            break;
        }
        printf("[%s] -> RayGun Event: %s\n", did_key, result_event.data);
        free(result_event.data);
    }
    rayguneventstream_free(stream);
    free(event_stream);
    return NULL;
}

void *message_stream_thread(void *arg)
{
    MsgEventStream *event_stream = arg;
    MessageEventStream *stream = event_stream->stream;
    char *did_key = did_to_string(event_stream->did);
    char *conversation_id = event_stream->conversation_id;
    while (1)
    {
        FFIResult_String result_event = message_stream_next(stream);
        if (result_event.error)
        {
            print_error(result_event.error);
            break;
        }
        printf("[CoID:%s][%s] -> Messaging Event: %s\n", conversation_id, did_key, result_event.data);
        free(result_event.data);
    }
    messageeventstream_free(stream);
    free(event_stream);
    return NULL;
}

MultiPassAdapter *new_account(const char *pass)
{

    Tesseract *tesseract = tesseract_new();

    FFIResult_Null result_unlock_t = tesseract_unlock(tesseract, pass);
    if (result_unlock_t.error)
    {
        print_error(result_unlock_t.error);
        return NULL;
    }

    MpIpfsConfig *config = mp_ipfs_config_development();

    FFIResult_MultiPassAdapter result_mp = multipass_mp_ipfs_temporary(NULL, tesseract, config);

    if (result_mp.error)
    {
        print_error(result_mp.error);
        return NULL;
    }

    tesseract_free(tesseract);
    FFIResult_DID result_ignore_t = multipass_create_identity(result_mp.data, NULL, NULL);
    if (result_ignore_t.error)
    {
        printf("Error creating identity\n");
        print_error(result_ignore_t.error);
        return NULL;
    }
    free(config);
    return result_mp.data;
}

RayGunAdapter *new_chat(const MultiPassAdapter *mp)
{
    RgIpfsConfig *config = rg_ipfs_config_development();
    FFIResult_RayGunAdapter result_rg_t = warp_rg_ipfs_temporary_new(mp, NULL, config);
    if (result_rg_t.error)
    {
        print_error(result_rg_t.error);
        return NULL;
    }
    free(config);
    return result_rg_t.data;
}

char *get_username(const MultiPassAdapter *account, const DID *id)
{
    if (!account || !id)
    {
        return "Null User";
    }

    Identifier *ident = multipass_identifier_did_key(id);
    if (!ident)
    {
        return "Null User";
    }

    FFIResult_FFIVec_Identity result_identity_t = multipass_get_identity(account, ident);
    if (result_identity_t.error)
    {
        print_error(result_identity_t.error);
        return "Null User";
    }

    if (result_identity_t.data->len == 0)
    {
        return did_to_string(id);
    }
    Identity *identity = result_identity_t.data->ptr[0];
    return multipass_identity_username(identity);
}

void print_messages(const MultiPassAdapter *account, const FFIVec_Message *messages)
{

    for (uintptr_t i = 0; i < messages->len; i++)
    {
        Message *message = messages->ptr[i];
        if (!message)
        {
            printf("Message is null\n");
            return;
        }

        DID *sender = message_sender(message);
        char *username = get_username(account, sender);

        FFIVec_String *messages_vec = message_lines(message);

        if (messages_vec->len >= 1)
        {
            for (uintptr_t x = 0; x < messages_vec->len; x++)
            {
                char *line = messages_vec->ptr[x];
                if (line)
                    printf("%lu: %s: %s\n", x, username, line);
            }
        }
    }
}

void spawn_stream(MultiPassAdapter *mp, RayGunAdapter *rg)
{
    FFIResult_Identity result_ident = multipass_get_own_identity(mp);
    if (result_ident.error)
    {
        print_error(result_ident.error);
        return;
    }

    DID *did = multipass_identity_did_key(result_ident.data);

    FFIResult_RayGunEventStream result_stream = raygun_subscribe(rg);

    if (result_stream.error)
    {
        print_error(result_stream.error);
        return; // Do we want to return NULL?
    }

    RGEventStream event_stream = {.did = did, .stream = result_stream.data};
    thrd_t thr;
    thrd_create(&thr, (thrd_start_t)raygun_stream_thread, (void *)&event_stream);
}

int main()
{
    MultiPassAdapter *account_a = new_account("c_datastore_a");
    MultiPassAdapter *account_b = new_account("c_datastore_b");

    if (!account_a || !account_b)
    {
        printf("Error creating account\n");
        return -1;
    }
    FFIResult_Identity result_ident_b = multipass_get_own_identity(account_b);

    if (result_ident_b.error)
    {
        print_error(result_ident_b.error);
        return -1;
    }

    Identity *ident_b = result_ident_b.data;

    RayGunAdapter *chatter_a = new_chat(account_a);
    // spawn_stream(account_a, chatter_a);
    RayGunAdapter *chatter_b = new_chat(account_b);
    // spawn_stream(account_b, chatter_b);

    if (!chatter_a || !chatter_b)
    {
        printf("Error creating account\n");
        return -1;
    }

    sleep(1);
    sleep(1);
    DID *ident_b_did = multipass_identity_did_key(ident_b);
    FFIResult_Conversation result_convo = raygun_create_conversation(chatter_a, ident_b_did);
    if (result_convo.error)
    {
        print_error(result_convo.error);
        return -1;
    }

    char *conversation = conversation_id(result_convo.data);
    sleep(1);
    sleep(1);
    // Messages are sent to rust via ffi as a array pointer. We would
    const char *chat_a_message[] = {"How are you??"};

    int message_lines_length = sizeof(chat_a_message) / sizeof(chat_a_message[0]);

    FFIResult_Null result_chat_t = raygun_send(chatter_a, conversation, NULL, chat_a_message, message_lines_length);

    if (result_chat_t.error)
    {
        print_error(result_chat_t.error);
        return -1;
    }
    // Because its async and processing internally, we want to make sure that chatter_b to receive it before attempting to .
    sleep(1);

    FFIResult_FFIVec_Message result_messages_b_t = raygun_get_messages(chatter_b, conversation, NULL);

    if (result_messages_b_t.error)
    {
        print_error(result_messages_b_t.error);
        return -1;
    }

    printf("Chatter B messages\n");
    print_messages(account_b, result_messages_b_t.data);
    printf("\n");

    // Messages are sent to rust via ffi as a array pointer. We would
    const char *chat_b_message[] = {"Hello from Chatter A :D"};

    int message_b_lines_length = sizeof(chat_b_message) / sizeof(chat_b_message[0]);
    FFIResult_Null result_chat_b_t = raygun_send(chatter_b, conversation, NULL, chat_b_message, message_b_lines_length);

    if (result_chat_b_t.error)
    {
        print_error(result_chat_b_t.error);
        return -1;
    }

    // Ditto; We may implement a small "wait" in rust to make sure the peers are synced up in some manner.
    sleep(1);

    // Note: Because chatter a already had messages stored that was previously sent
    FFIResult_FFIVec_Message result_messages_t = raygun_get_messages(chatter_a, conversation, NULL);

    if (result_messages_t.error)
    {
        print_error(result_messages_t.error);
        return -1;
    }

    printf("Chatter A messages\n");
    print_messages(account_a, result_messages_t.data);

    return 0;
}
