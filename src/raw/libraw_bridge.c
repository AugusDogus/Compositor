// A small C boundary keeps LibRaw's versioned public structs out of Rust's ABI.
#include <libraw/libraw.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

typedef struct {
    libraw_processed_image_t *image;
    const uint16_t *pixels;
    size_t samples;
    uint32_t width, height, bits;
    float as_shot[3], camera_to_rgb[9], xyz_to_camera[9];
    float iso, aperture, shutter, focal_length;
    char camera[256], lens[128];
} CompositorRaw;

void compositor_raw_free(CompositorRaw *out) {
    if (out->image) libraw_dcraw_clear_mem(out->image);
    out->image = NULL;
    out->pixels = NULL;
}

int compositor_raw_decode(const uint8_t *bytes, size_t length, uint64_t max_pixels,
                         CompositorRaw *out, char *error, size_t error_size) {
    memset(out, 0, sizeof(*out));
    libraw_data_t *raw = libraw_init(0);
    if (!raw) { snprintf(error, error_size, "LibRaw could not allocate a decoder"); return -1; }
    raw->rawparams.max_raw_memory_mb = (unsigned)(max_pixels * 16 / (1024 * 1024));
    int code = libraw_open_buffer(raw, (void *)bytes, length);
    if (code) goto fail;
    if (raw->idata.is_foveon) {
        snprintf(error, error_size, "Foveon X3F color development is not supported; export a TIFF from Sigma Photo Pro");
        libraw_close(raw); return -1;
    }
    unsigned width = raw->sizes.width, height = raw->sizes.height;
    if (!width || !height || width > 30000 || height > 30000 || (uint64_t)width * height > max_pixels) {
        snprintf(error, error_size, "RAW dimensions exceed the 30000-pixel side or 200-megapixel surface limit");
        libraw_close(raw); return -1;
    }
    if (raw->idata.colors != 3) {
        snprintf(error, error_size, "LibRaw did not identify a three-color camera sensor");
        libraw_close(raw); return -1;
    }
    // Demosaic into linear camera RGB. White balance, color conversion and all
    // creative controls stay in Compositor's floating-point Develop pipeline.
    raw->params.output_color = 0;
    raw->params.output_bps = 16;
    raw->params.gamm[0] = raw->params.gamm[1] = 1.;
    raw->params.no_auto_bright = 1;
    raw->params.bright = 1.;
    raw->params.adjust_maximum_thr = 0.;
    raw->params.use_auto_wb = raw->params.use_camera_wb = 0;
    raw->params.user_qual = 3;
    for (int c = 0; c < 4; ++c) raw->params.user_mul[c] = 1.;
    for (int c = 0; c < 3; ++c) {
        out->as_shot[c] = raw->color.cam_mul[c];
        for (int j = 0; j < 3; ++j) {
            out->camera_to_rgb[c * 3 + j] = raw->color.rgb_cam[c][j];
            out->xyz_to_camera[c * 3 + j] = raw->color.cam_xyz[c][j];
        }
    }
    out->bits = raw->color.raw_bps;
    out->iso = raw->other.iso_speed;
    out->aperture = raw->other.aperture;
    out->shutter = raw->other.shutter;
    out->focal_length = raw->other.focal_len;
    snprintf(out->camera, sizeof(out->camera), "%.63s %.63s", raw->idata.make, raw->idata.model);
    snprintf(out->lens, sizeof(out->lens), "%.127s", raw->lens.Lens);
    code = libraw_unpack(raw);
    if (code) goto fail;
    code = libraw_dcraw_process(raw);
    if (code) goto fail;
    out->image = libraw_dcraw_make_mem_image(raw, &code);
    if (code || !out->image) goto fail;
    libraw_processed_image_t *image = out->image;
    if (image->type != LIBRAW_IMAGE_BITMAP || image->colors != 3 || image->bits != 16
        || !image->width || !image->height || image->width > 30000 || image->height > 30000
        || (uint64_t)image->width * image->height > max_pixels
        || (uint64_t)image->width * image->height * 6 != image->data_size) {
        snprintf(error, error_size, "LibRaw returned an invalid linear RGB surface");
        compositor_raw_free(out); libraw_close(raw); return -1;
    }
    out->width = image->width;
    out->height = image->height;
    out->samples = (size_t)image->width * image->height * 3;
    out->pixels = (const uint16_t *)image->data;
    libraw_close(raw);
    return 0;
fail:
    snprintf(error, error_size, "LibRaw: %s", code ? libraw_strerror(code) : "no decoded image was returned");
    compositor_raw_free(out);
    libraw_close(raw);
    return code ? code : -1;
}
