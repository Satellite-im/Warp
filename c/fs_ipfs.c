#include <stdint.h>
#include <stdio.h>
#include <stdarg.h>
#include <stdlib.h>
#include <string.h>
#include "../warp/warp.h"
#include "../extensions/warp-mp-ipfs/warp-mp-ipfs.h"
#include "../extensions/warp-fs-ipfs/warp-fs-ipfs.h"

void print_error(FFIError *error)
{
    printf("Error Type: %s\n", error->error_type);
    printf("Error Message: %s\n", error->error_message);
    ffierror_free(error);
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

    did_free(result_t.data);
    return mp;
}

int main()
{
    MultiPassAdapter *account = new_account();

    FFIResult_ConstellationAdapter result_fs = constellation_fs_ipfs_temporary_new(account);


    if (result_fs.error)
    {
        printf("Cannot create context for contellation\n");
        print_error(result_fs.error);
        return -1;
    }

    ConstellationAdapter *filesystem = result_fs.data;

    const char *data = "Hello, World!";
    uint32_t data_size = strlen(data);

    FFIResult_Null result_ignored_t = constellation_put_buffer(filesystem, "readme.txt", (const uint8_t *)data, data_size);
    if (result_ignored_t.error)
    {
        printf("Error uploading file\n");
        print_error(result_ignored_t.error);
        return -1;
    }

    FFIResult_FFIVec_u8 result_vu8_t = constellation_get_buffer(filesystem, "readme.txt");

    if (result_vu8_t.error)
    {
        print_error(result_vu8_t.error);
        return -1;
    }

    printf("%s\n", (const char *)result_vu8_t.data->ptr);

    constellationadapter_free(filesystem);

    return 0;
}