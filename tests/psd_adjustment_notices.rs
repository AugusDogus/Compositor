use compositor::{
    adjustment::ExtendedAdjustment,
    document::{Document, Layer, LayerContent},
    psd,
};

#[test]
fn supported_adjustments_import_without_metadata_loss_notices() {
    for adjustment in [
        ExtendedAdjustment::Threshold(Default::default()),
        ExtendedAdjustment::Posterize(Default::default()),
        ExtendedAdjustment::ChannelMixer(Default::default()),
        ExtendedAdjustment::SelectiveColor(Default::default()),
    ] {
        let mut document = Document::new(1, 1).unwrap();
        compositor::edits::fill(&mut document, [80, 120, 160, 255], false, false).unwrap();
        let mut layer = Layer::blank(adjustment.label(), 1, 1);
        layer.content = LayerContent::ExtendedAdjustment(Box::new(adjustment));
        document.add(layer).unwrap();
        let imported = psd::decode(&psd::encode(&document).unwrap()).unwrap();
        assert_eq!(
            imported.document.layers[1].content,
            document.layers[1].content
        );
        assert!(
            imported
                .report
                .changes
                .iter()
                .all(|notice| !notice.contains("not preserved as editable metadata")),
            "{}: {}",
            adjustment.label(),
            imported.report.description()
        );
    }
}

#[test]
fn malformed_selective_color_record_is_rejected_instead_of_losing_the_adjustment() {
    let mut document = Document::new(1, 1).unwrap();
    let mut layer = Layer::blank("Selective Color", 1, 1);
    layer.content = LayerContent::ExtendedAdjustment(Box::new(ExtendedAdjustment::SelectiveColor(
        Default::default(),
    )));
    document.add(layer).unwrap();
    let bytes = psd::encode(&document).unwrap();
    let record = bytes.windows(8).position(|v| v == b"8BIMselc").unwrap();
    // Signature, key and payload length precede the two-byte version.
    let version = record + 12;
    assert_eq!(&bytes[version..version + 2], &[0, 1]);
    for offset in [version + 1, version + 3] {
        let mut malformed = bytes.clone();
        malformed[offset] = 2;
        let error = psd::decode(&malformed)
            .err()
            .expect("invalid record must fail");
        assert!(error.to_string().contains("selc"), "{error}");
    }
}
