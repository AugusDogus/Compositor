# Lab import reference

`pillow-cielab.tiff` contains a 48×32 RGB gradient converted to CIELab with
Pillow ImageCms and saved with its Lab ICC profile. `pillow-srgb.png` converts
that decoded TIFF back to sRGB using Little CMS without lookup-table
approximation. Disabling that approximation matters near the gamut boundary.

Generated with Pillow 12.3.0 (the fixture is independent of the Rust decoder):

```python
from io import BytesIO
from PIL import Image, ImageCms

rgb = Image.new("RGB", (48, 32))
rgb.putdata([(x * 5 % 256, y * 7 % 256, (x + y) * 3 % 256)
             for y in range(32) for x in range(48)])
srgb, lab = ImageCms.createProfile("sRGB"), ImageCms.createProfile("LAB")
encoded = ImageCms.profileToProfile(rgb, srgb, lab, outputMode="LAB")
encoded.save("pillow-cielab.tiff",
             icc_profile=ImageCms.ImageCmsProfile(lab).tobytes())
image = Image.open("pillow-cielab.tiff")
profile = ImageCms.ImageCmsProfile(BytesIO(image.info["icc_profile"]))
ImageCms.profileToProfile(image, profile, srgb, outputMode="RGB",
                         flags=ImageCms.Flags.NOOPTIMIZE).save("pillow-srgb.png")
```

Channel encodings follow [Adobe Photoshop TIFF Technical Notes, March 2002,
pages 12–13](https://download.osgeo.org/libtiff/doc/TIFFphotoshop.pdf).
CIELab16 lightness uses 0–65535; ICCLab16 uses 0–65280. Both encode a/b in
units of 1/256, signed for CIELab and offset by 32768 for ICCLab.
