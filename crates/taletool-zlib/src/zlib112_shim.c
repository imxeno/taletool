#include <string.h>

#include "zlib.h"

#define TALETOOL_ZLIB112_DEFAULT_MEM_LEVEL 8

uLong taletool_zlib112_compress_bound(uLong source_len) {
    return source_len + (source_len >> 12) + (source_len >> 14) + 11;
}

int taletool_zlib112_compress(
    Bytef *dest,
    uLongf *dest_len,
    const Bytef *source,
    uLong source_len,
    int level,
    int strategy
) {
    z_stream stream;
    int err;

    if ((uLong)(uInt)source_len != source_len ||
        (uLong)(uInt)(*dest_len) != *dest_len) {
        return Z_BUF_ERROR;
    }

    memset(&stream, 0, sizeof(stream));
    stream.next_in = (Bytef *)source;
    stream.avail_in = (uInt)source_len;
    stream.next_out = dest;
    stream.avail_out = (uInt)(*dest_len);

    err = deflateInit2(
        &stream,
        level,
        Z_DEFLATED,
        MAX_WBITS,
        TALETOOL_ZLIB112_DEFAULT_MEM_LEVEL,
        strategy
    );
    if (err != Z_OK) {
        return err;
    }

    err = deflate(&stream, Z_FINISH);
    if (err != Z_STREAM_END) {
        deflateEnd(&stream);
        return err == Z_OK ? Z_BUF_ERROR : err;
    }

    *dest_len = stream.total_out;
    err = deflateEnd(&stream);
    return err == Z_OK ? Z_OK : err;
}
