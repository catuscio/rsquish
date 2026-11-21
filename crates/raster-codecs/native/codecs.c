#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <setjmp.h>
#include <limits.h>
#include <jpeglib.h>
#include <webp/encode.h>
#include <avif/avif.h>

#define RK_MAX_PIXELS (64ULL * 1024ULL * 1024ULL)

static int fail(char *error, size_t capacity, const char *message) {
    if (error && capacity) snprintf(error, capacity, "%s", message);
    return 0;
}

void rk_free(uint8_t *data) { free(data); }

struct jpeg_error {
    struct jpeg_error_mgr base;
    jmp_buf jump;
    char message[JMSG_LENGTH_MAX];
};
struct jpeg_context {
    struct jpeg_compress_struct info;
    struct jpeg_error error;
    unsigned char *bytes;
    unsigned long size;
    int created;
};
static void jpeg_failure(j_common_ptr info) {
    struct jpeg_error *error = (struct jpeg_error *)info->err;
    (*info->err->format_message)(info, error->message);
    longjmp(error->jump, 1);
}

static int encode_jpeg(const uint8_t *pixels, uint32_t width, uint32_t height,
                       int quality, uint8_t **out, size_t *out_len,
                       char *error, size_t capacity) {
    // State lives on the heap so longjmp cannot invalidate modified locals.
    struct jpeg_context *context = calloc(1, sizeof(*context));
    if (!context) return fail(error, capacity, "JPEG context allocation failed");
    context->info.err = jpeg_std_error(&context->error.base);
    context->error.base.error_exit = jpeg_failure;
    if (setjmp(context->error.jump)) {
        fail(error, capacity, context->error.message);
        if (context->created) jpeg_destroy_compress(&context->info);
        free(context->bytes);
        free(context);
        return 0;
    }
    // jpeg_destroy_compress can clean a partially initialized, zeroed object.
    context->created = 1;
    jpeg_create_compress(&context->info);
    jpeg_mem_dest(&context->info, &context->bytes, &context->size);
    context->info.image_width = width;
    context->info.image_height = height;
    context->info.input_components = 3;
    context->info.in_color_space = JCS_RGB;
    jpeg_set_defaults(&context->info);
    jpeg_set_quality(&context->info, quality, TRUE);
    context->info.comp_info[0].h_samp_factor = 2;
    context->info.comp_info[0].v_samp_factor = 2;
    context->info.comp_info[1].h_samp_factor = 1;
    context->info.comp_info[1].v_samp_factor = 1;
    context->info.comp_info[2].h_samp_factor = 1;
    context->info.comp_info[2].v_samp_factor = 1;
    jpeg_simple_progression(&context->info);
    context->info.optimize_coding = TRUE;
    jpeg_start_compress(&context->info, TRUE);
    while (context->info.next_scanline < context->info.image_height) {
        JSAMPROW row = (JSAMPROW)(pixels + (size_t)context->info.next_scanline * width * 3);
        if (jpeg_write_scanlines(&context->info, &row, 1) != 1) {
            jpeg_destroy_compress(&context->info);
            free(context->bytes);
            free(context);
            return fail(error, capacity, "JPEG encoder failed to consume input row");
        }
    }
    jpeg_finish_compress(&context->info);
    jpeg_destroy_compress(&context->info);
    *out = context->bytes;
    *out_len = (size_t)context->size;
    free(context);
    return 1;
}

static int encode_webp(const uint8_t *pixels, uint32_t width, uint32_t height,
                       int quality, uint8_t **out, size_t *out_len,
                       char *error, size_t capacity) {
    WebPConfig config;
    WebPPicture picture;
    WebPMemoryWriter writer;
    if (!WebPConfigInit(&config) || !WebPPictureInit(&picture))
        return fail(error, capacity, "WebP ABI initialization failed");
    config.quality = (float)quality;
    config.method = 6;
    config.thread_level = 1;
    config.lossless = 0;
    if (!WebPValidateConfig(&config)) return fail(error, capacity, "Invalid WebP configuration");
    picture.width = (int)width;
    picture.height = (int)height;
    picture.use_argb = 0;
    WebPMemoryWriterInit(&writer);
    picture.writer = WebPMemoryWrite;
    picture.custom_ptr = &writer;
    if (!WebPPictureImportRGB(&picture, pixels, (int)(width * 3))) {
        WebPPictureFree(&picture);
        WebPMemoryWriterClear(&writer);
        return fail(error, capacity, "WebP RGB import failed (check image dimension limit)");
    }
    if (!WebPEncode(&config, &picture)) {
        char message[128];
        snprintf(message, sizeof(message), "WebP encoding failed with error %d", picture.error_code);
        WebPPictureFree(&picture);
        WebPMemoryWriterClear(&writer);
        return fail(error, capacity, message);
    }
    uint8_t *copy = malloc(writer.size);
    if (!copy) {
        WebPPictureFree(&picture);
        WebPMemoryWriterClear(&writer);
        return fail(error, capacity, "WebP result allocation failed");
    }
    memcpy(copy, writer.mem, writer.size);
    *out = copy;
    *out_len = writer.size;
    WebPPictureFree(&picture);
    WebPMemoryWriterClear(&writer);
    return 1;
}

void *rk_avif_prepare(const uint8_t *pixels, size_t len, uint32_t width, uint32_t height,
                      char *error, size_t capacity) {
    const uint64_t count = (uint64_t)width * height;
    if (!pixels || !width || !height || width > 32768 || height > 32768 || count > RK_MAX_PIXELS || count * 3 != len) {
        fail(error, capacity, "Invalid RGB8 dimensions/buffer length");
        return NULL;
    }
    avifImage *image = avifImageCreate(width, height, 8, AVIF_PIXEL_FORMAT_YUV420);
    if (!image) { fail(error, capacity, "AVIF image allocation failed"); return NULL; }
    image->yuvRange = AVIF_RANGE_FULL;
    image->colorPrimaries = AVIF_COLOR_PRIMARIES_BT709;
    image->transferCharacteristics = AVIF_TRANSFER_CHARACTERISTICS_SRGB;
    image->matrixCoefficients = AVIF_MATRIX_COEFFICIENTS_BT601;
    avifRGBImage rgb;
    avifRGBImageSetDefaults(&rgb, image);
    rgb.format = AVIF_RGB_FORMAT_RGB;
    rgb.depth = 8;
    rgb.maxThreads = 1;
    rgb.pixels = (uint8_t *)pixels;
    rgb.rowBytes = width * 3;
    avifResult result = avifImageRGBToYUV(image, &rgb);
    if (result != AVIF_RESULT_OK) {
        fail(error, capacity, avifResultToString(result));
        avifImageDestroy(image);
        return NULL;
    }
    return image;
}

void rk_avif_source_free(void *source) {
    if (source) avifImageDestroy((avifImage *)source);
}

static int encode_avif_source(const avifImage *image, int quality, const char *tune, int threads,
                             uint8_t **out, size_t *out_len, char *error, size_t capacity) {
    avifEncoder *encoder = avifEncoderCreate();
    avifRWData bytes = AVIF_DATA_EMPTY;
    int ok = 0;
    if (!encoder) return fail(error, capacity, "AVIF encoder allocation failed");
    avifResult result;
    encoder->codecChoice = AVIF_CODEC_CHOICE_AOM;
    encoder->maxThreads = threads;
    encoder->speed = 6;
    encoder->quality = quality;
    encoder->qualityAlpha = 100;
    encoder->tileRowsLog2 = 0;
    encoder->tileColsLog2 = 0;
    encoder->autoTiling = AVIF_FALSE;
    result = avifEncoderSetCodecSpecificOption(encoder, "color:tune", tune);
    if (result == AVIF_RESULT_OK) result = avifEncoderWrite(encoder, image, &bytes);
    if (result != AVIF_RESULT_OK) {
        char message[384];
        snprintf(message, sizeof(message), "%s: %.240s", avifResultToString(result), encoder->diag.error);
        fail(error, capacity, message);
        goto cleanup;
    }
    *out = malloc(bytes.size);
    if (!*out) {
        fail(error, capacity, "AVIF result allocation failed");
        goto cleanup;
    }
    memcpy(*out, bytes.data, bytes.size);
    *out_len = bytes.size;
    ok = 1;
cleanup:
    avifRWDataFree(&bytes);
    if (encoder) avifEncoderDestroy(encoder);
    return ok;
}

int rk_avif_encode_prepared(const void *source, int codec, int quality,
                            uint8_t **out, size_t *out_len, char *error, size_t capacity) {
    if (!out || !out_len) return fail(error, capacity, "Invalid output pointers");
    *out = NULL; *out_len = 0;
    if (!source || (codec != 2 && codec != 3) || quality < 0 || quality > 100)
        return fail(error, capacity, "Invalid prepared AVIF parameters");
    return encode_avif_source(source, quality, codec == 2 ? "iq" : "ssim", 1, out, out_len, error, capacity);
}

static int encode_avif(const uint8_t *pixels, uint32_t width, uint32_t height,
                       int quality, const char *tune, uint8_t **out, size_t *out_len,
                       char *error, size_t capacity) {
    void *source = rk_avif_prepare(pixels, (size_t)width * height * 3, width, height, error, capacity);
    if (!source) return 0;
    int result = encode_avif_source(source, quality, tune, 1, out, out_len, error, capacity);
    rk_avif_source_free(source);
    return result;
}

int rk_encode(int codec, const uint8_t *pixels, size_t len, uint32_t width, uint32_t height,
               int quality, uint8_t **out, size_t *out_len, char *error, size_t capacity) {
    if (!out || !out_len) return fail(error, capacity, "Invalid output pointers");
    *out = NULL;
    *out_len = 0;
    const uint64_t count = (uint64_t)width * height;
    if (!pixels || !width || !height || width > 32768 || height > 32768 || count > RK_MAX_PIXELS || count * 3 != len)
        return fail(error, capacity, "Invalid RGB8 dimensions/buffer length");
    if (quality < 0 || quality > 100) return fail(error, capacity, "Quality must be 0..100");
    switch (codec) {
        case 0: return encode_jpeg(pixels, width, height, quality, out, out_len, error, capacity);
        case 1: return encode_webp(pixels, width, height, quality, out, out_len, error, capacity);
        case 2: return encode_avif(pixels, width, height, quality, "iq", out, out_len, error, capacity);
        case 3: return encode_avif(pixels, width, height, quality, "ssim", out, out_len, error, capacity);
        default: return fail(error, capacity, "Unknown codec");
    }
}

int rk_decode_avif(const uint8_t *data, size_t len, uint8_t **out, size_t *out_len,
                    uint32_t *width, uint32_t *height, char *error, size_t capacity) {
    if (!out || !out_len || !width || !height) return fail(error, capacity, "Invalid output pointers");
    *out = NULL; *out_len = 0; *width = 0; *height = 0;
    if (!data || !len || len > 256ULL * 1024ULL * 1024ULL) return fail(error, capacity, "Invalid AVIF input size");
    avifDecoder *decoder = avifDecoderCreate();
    avifImage *image = avifImageCreateEmpty();
    int ok = 0;
    uint8_t *pixels = NULL;
    if (!decoder || !image) {
        fail(error, capacity, "AVIF decoder allocation failed");
        goto cleanup;
    }
    decoder->codecChoice = AVIF_CODEC_CHOICE_AOM;
    decoder->maxThreads = 1;
    decoder->ignoreExif = AVIF_TRUE;
    decoder->ignoreXMP = AVIF_TRUE;
    decoder->imageSizeLimit = (uint32_t)RK_MAX_PIXELS;
    decoder->imageDimensionLimit = 32768;
    decoder->imageCountLimit = 1;
    avifResult result = avifDecoderReadMemory(decoder, image, data, len);
    if (result != AVIF_RESULT_OK) {
        fail(error, capacity, avifResultToString(result));
        goto cleanup;
    }
    if (image->alphaPlane || image->transformFlags || decoder->imageCount > 1 || image->icc.size) {
        fail(error, capacity, "AVIF RGB8 decode does not support alpha, transforms, animation, or embedded ICC profiles");
        goto cleanup;
    }
    if ((image->colorPrimaries != AVIF_COLOR_PRIMARIES_BT709 && image->colorPrimaries != AVIF_COLOR_PRIMARIES_UNSPECIFIED)
        || (image->transferCharacteristics != AVIF_TRANSFER_CHARACTERISTICS_SRGB
            && image->transferCharacteristics != AVIF_TRANSFER_CHARACTERISTICS_UNSPECIFIED)) {
        fail(error, capacity, "AVIF RGB8 decode requires sRGB/unspecified color primaries and transfer");
        goto cleanup;
    }
    const uint64_t count = (uint64_t)image->width * image->height;
    if (!count || count > RK_MAX_PIXELS || image->width > 32768 || image->height > 32768) {
        fail(error, capacity, "AVIF decoded dimensions exceed limits");
        goto cleanup;
    }
    pixels = malloc((size_t)count * 3);
    if (!pixels) {
        fail(error, capacity, "AVIF RGB buffer allocation failed");
        goto cleanup;
    }
    avifRGBImage rgb;
    avifRGBImageSetDefaults(&rgb, image);
    rgb.depth = 8;
    rgb.format = AVIF_RGB_FORMAT_RGB;
    rgb.maxThreads = 1;
    rgb.rowBytes = image->width * 3;
    rgb.pixels = pixels;
    result = avifImageYUVToRGB(image, &rgb);
    if (result != AVIF_RESULT_OK) {
        fail(error, capacity, avifResultToString(result));
        goto cleanup;
    }
    *out = pixels; pixels = NULL;
    *out_len = (size_t)count * 3;
    *width = image->width; *height = image->height;
    ok = 1;
cleanup:
    free(pixels);
    if (image) avifImageDestroy(image);
    if (decoder) avifDecoderDestroy(decoder);
    return ok;
}
