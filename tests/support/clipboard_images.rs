pub fn profiled_png() -> Vec<u8> {
    let mut info = png::Info::with_size(1, 1);
    info.color_type = png::ColorType::GrayscaleAlpha;
    info.bit_depth = png::BitDepth::Eight;
    info.icc_profile = Some(std::borrow::Cow::Owned(
        lcms2::Profile::new_gray(lcms2::CIExyY::d50(), &lcms2::ToneCurve::new(1.))
            .unwrap()
            .icc()
            .unwrap(),
    ));
    let mut bytes = Vec::new();
    let mut writer = png::Encoder::with_info(&mut bytes, info)
        .unwrap()
        .write_header()
        .unwrap();
    writer.write_image_data(&[128, 123]).unwrap();
    writer.finish().unwrap();
    bytes
}

pub fn oriented_tiff() -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    {
        let mut encoder = tiff::encoder::TiffEncoder::new(&mut bytes).unwrap();
        let mut image = encoder
            .new_image::<tiff::encoder::colortype::RGB8>(7, 3)
            .unwrap();
        image
            .encoder()
            .write_tag(tiff::tags::Tag::Orientation, 6_u16)
            .unwrap();
        image.write_data(&[128; 63]).unwrap();
    }
    bytes.into_inner()
}
