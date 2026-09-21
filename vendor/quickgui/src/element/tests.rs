use super::*;
use taffy::{AvailableSpace, TaffyTree, style::RepetitionCount};

#[test]
fn accessibility_relations_are_exact_and_cost_one_pointer_when_absent() {
    assert!(
        std::mem::size_of::<AccessibilityRelationsStyle>()
            <= std::mem::size_of::<Option<ElementId>>()
    );
    let element = div()
        .accessibility_controls(0_u64)
        .accessibility_active_descendant(u64::MAX)
        .accessibility_labelled_by(42_u64)
        .accessibility_described_by_pair(43_u64, 44_u64)
        .required(true);
    assert_eq!(
        element.accessibility.relations.controls(),
        Some(ElementId::new(0))
    );
    assert_eq!(
        element.accessibility.relations.active_descendant(),
        Some(ElementId::new(u64::MAX))
    );
    assert_eq!(
        element.accessibility.relations.labelled_by(),
        Some(ElementId::new(42))
    );
    assert_eq!(
        element.accessibility.relations.described_by(),
        Some(ElementId::new(43))
    );
    assert_eq!(
        element.accessibility.relations.described_by_secondary(),
        Some(ElementId::new(44))
    );
    assert!(element.accessibility.required);

    let replaced = element.accessibility_described_by(45_u64);
    assert_eq!(
        replaced.accessibility.relations.described_by(),
        Some(ElementId::new(45))
    );
    assert_eq!(
        replaced.accessibility.relations.described_by_secondary(),
        None
    );
}

#[test]
fn tailwind_spacing_uses_four_pixel_units() {
    let element = div().p_4().gap_2().h_8();
    assert_eq!(element.layout.size.height, Dimension::length(32.0));
    assert_eq!(element.layout.padding.left, LengthPercentage::length(16.0));
    assert_eq!(element.layout.gap.width, LengthPercentage::length(8.0));
}

#[test]
fn flex_helpers_match_gpui_and_css_shorthands() {
    let row = div()
        .flex_row_reverse()
        .flex_auto()
        .flex_basis(48.0)
        .flex_grow(f32::NAN)
        .flex_shrink(f32::INFINITY)
        .items_baseline()
        .justify_evenly()
        .content_around();
    assert_eq!(row.layout.display, Display::Flex);
    assert_eq!(row.layout.flex_direction, FlexDirection::RowReverse);
    assert_eq!(row.layout.flex_basis, Dimension::length(48.0));
    assert_eq!(row.layout.flex_grow, 0.0);
    assert_eq!(row.layout.flex_shrink, 0.0);
    assert_eq!(row.layout.align_items, Some(AlignItems::BASELINE));
    assert_eq!(
        row.layout.justify_content,
        Some(JustifyContent::SPACE_EVENLY)
    );
    assert_eq!(row.layout.align_content, Some(AlignContent::SPACE_AROUND));

    let item = div().flex_initial().self_end().aspect_square();
    assert_eq!(item.layout.flex_grow, 0.0);
    assert_eq!(item.layout.flex_shrink, 1.0);
    assert_eq!(item.layout.flex_basis, Dimension::auto());
    assert_eq!(item.layout.align_self, Some(AlignSelf::END));
    assert_eq!(item.layout.aspect_ratio, Some(1.0));
    assert_eq!(div().aspect_ratio(0.0).layout.aspect_ratio, None);
    assert_eq!(div().aspect_ratio(f32::NAN).layout.aspect_ratio, None);
}

#[test]
fn margin_and_axis_gap_helpers_are_finite_and_web_shaped() {
    let element = div().mx_4().mt(-8.0).mb(f32::NAN).gap_x_3().gap_y(10.0);
    assert_eq!(
        element.layout.margin.left,
        LengthPercentageAuto::length(16.0)
    );
    assert_eq!(
        element.layout.margin.right,
        LengthPercentageAuto::length(16.0)
    );
    assert_eq!(
        element.layout.margin.top,
        LengthPercentageAuto::length(-8.0)
    );
    assert_eq!(
        element.layout.margin.bottom,
        LengthPercentageAuto::length(0.0)
    );
    assert_eq!(element.layout.gap.width, LengthPercentage::length(12.0));
    assert_eq!(element.layout.gap.height, LengthPercentage::length(10.0));

    let centered = div().mx_auto();
    assert_eq!(centered.layout.margin.left, LengthPercentageAuto::auto());
    assert_eq!(centered.layout.margin.right, LengthPercentageAuto::auto());
}

#[test]
fn flex_reverse_aspect_and_auto_margins_reach_taffy_layout() {
    let children = [div().w(50.0).h(20.0), div().w(40.0).aspect_square()];
    let mut taffy = TaffyTree::<()>::new();
    let child_nodes = children
        .iter()
        .map(|child| taffy.new_leaf(child.layout.clone()).unwrap())
        .collect::<Vec<_>>();
    let root = div().flex_row_reverse().items_start().size(200.0, 100.0);
    let root_node = taffy.new_with_children(root.layout, &child_nodes).unwrap();
    taffy
        .compute_layout(
            root_node,
            TaffySize {
                width: AvailableSpace::Definite(200.0),
                height: AvailableSpace::Definite(100.0),
            },
        )
        .unwrap();

    let first = taffy.layout(child_nodes[0]).unwrap();
    let second = taffy.layout(child_nodes[1]).unwrap();
    assert_eq!((first.location.x, first.size.width), (150.0, 50.0));
    assert_eq!((second.size.width, second.size.height), (40.0, 40.0));
    assert!(second.location.x < first.location.x);

    let centered = div().w(40.0).aspect_square().mx_auto();
    let centered_node = taffy.new_leaf(centered.layout).unwrap();
    let block = div().block().size(200.0, 100.0);
    let block_node = taffy
        .new_with_children(block.layout, &[centered_node])
        .unwrap();
    taffy
        .compute_layout(
            block_node,
            TaffySize {
                width: AvailableSpace::Definite(200.0),
                height: AvailableSpace::Definite(100.0),
            },
        )
        .unwrap();
    let centered = taffy.layout(centered_node).unwrap();
    assert_eq!(centered.location.x, 80.0);
    assert_eq!((centered.size.width, centered.size.height), (40.0, 40.0));
}

#[test]
fn forms_and_submit_buttons_keep_web_semantics_explicit() {
    let form = form();
    let submit = submit_button();

    assert!(form.form);
    assert_eq!(form.accessibility.role, AccessibilityRole::Form);
    assert!(submit.form_submitter);
    assert!(submit.clickable);
    assert!(submit.focusable);
    assert_eq!(submit.accessibility.role, AccessibilityRole::Button);
}

#[test]
fn validation_messages_are_utf8_safe_and_bounded() {
    let message = "你".repeat(MAX_VALIDATION_MESSAGE_BYTES);
    let element = text_input("").validation_message(message);
    let retained = element
        .accessibility
        .validation_message
        .as_deref()
        .expect("bounded validation message");

    assert!(retained.len() <= MAX_VALIDATION_MESSAGE_BYTES);
    assert!(retained.is_char_boundary(retained.len()));
    assert!(element.accessibility.validation_message_truncated);
}

#[test]
fn axis_padding_utilities_compose_like_tailwind() {
    let element = div().px_4().py_2();
    assert_eq!(element.layout.padding.left, LengthPercentage::length(16.0));
    assert_eq!(element.layout.padding.right, LengthPercentage::length(16.0));
    assert_eq!(element.layout.padding.top, LengthPercentage::length(8.0));
    assert_eq!(element.layout.padding.bottom, LengthPercentage::length(8.0));
}

#[test]
fn border_edge_widths_update_paint_and_layout_independently() {
    let color = Color::rgb8(71, 85, 105);
    let element = div()
        .border(1.0, color)
        .border_top_width(2.0)
        .border_right_width(3.0)
        .border_bottom_width(4.0)
        .border_left_width(5.0);

    assert_eq!(
        element.visual.border_widths,
        Insets {
            top: 2.0,
            right: 3.0,
            bottom: 4.0,
            left: 5.0,
        }
    );
    assert_eq!(element.visual.border_color, Some(color));
    assert_eq!(element.layout.border.top, LengthPercentage::length(2.0));
    assert_eq!(element.layout.border.right, LengthPercentage::length(3.0));
    assert_eq!(element.layout.border.bottom, LengthPercentage::length(4.0));
    assert_eq!(element.layout.border.left, LengthPercentage::length(5.0));

    let reset = element.border(6.0, Color::WHITE);
    assert_eq!(reset.visual.border_widths, Insets::all(6.0));
}

#[test]
fn grid_helpers_match_gpui_tracks_and_css_placements() {
    let grid = div()
        .grid()
        .grid_cols(5)
        .grid_rows_min_content(3)
        .grid_flow_col_dense();
    assert_eq!(grid.layout.display, Display::Grid);
    assert_eq!(grid.layout.grid_auto_flow, GridAutoFlow::ColumnDense);

    let GridTemplateComponent::Repeat(columns) = &grid.layout.grid_template_columns[0] else {
        panic!("equal columns should retain one compact repeat component");
    };
    assert_eq!(columns.count, RepetitionCount::Count(5));
    let expected_column: TrackSizingFunction = minmax(length(0.0_f32), fr(1.0_f32));
    assert_eq!(columns.tracks, [expected_column]);

    let GridTemplateComponent::Repeat(rows) = &grid.layout.grid_template_rows[0] else {
        panic!("equal rows should retain one compact repeat component");
    };
    assert_eq!(rows.count, RepetitionCount::Count(3));
    let expected_row: TrackSizingFunction = minmax(min_content(), fr(1.0_f32));
    assert_eq!(rows.tracks, [expected_row]);

    let item = div().col_start(2).col_end(-2).row_span(3).row_start_auto();
    assert!(
        matches!(item.layout.grid_column.start, GridPlacement::Line(line) if line.as_i16() == 2)
    );
    assert!(
        matches!(item.layout.grid_column.end, GridPlacement::Line(line) if line.as_i16() == -2)
    );
    assert_eq!(item.layout.grid_row.start, GridPlacement::Auto);
    assert_eq!(item.layout.grid_row.end, GridPlacement::Span(3));
}

#[test]
fn grid_templates_and_placements_are_hard_bounded() {
    let grid = div()
        .grid_cols(u16::MAX)
        .grid_template_rows(std::iter::repeat_n(
            GridTrack::fr(1.0),
            usize::from(MAX_GRID_TRACKS) + 50,
        ));
    let GridTemplateComponent::Repeat(columns) = &grid.layout.grid_template_columns[0] else {
        panic!("equal columns should use repeat");
    };
    assert_eq!(columns.count, RepetitionCount::Count(MAX_GRID_TRACKS));
    assert_eq!(
        grid.layout.grid_template_rows.len(),
        usize::from(MAX_GRID_TRACKS)
    );

    let item = div()
        .col_start(i16::MAX)
        .row_end(i16::MIN)
        .col_span(u16::MAX);
    assert_eq!(
        item.layout.grid_column,
        TaffyLine {
            start: GridPlacement::Span(MAX_GRID_TRACKS),
            end: GridPlacement::Span(MAX_GRID_TRACKS),
        }
    );
    assert!(
        matches!(item.layout.grid_row.end, GridPlacement::Line(line) if line.as_i16() == -MAX_GRID_LINE)
    );
    assert_eq!(GridTrack::px(f32::NAN), GridTrack::px(0.0));
    assert_eq!(GridTrack::fr(f32::NEG_INFINITY), GridTrack::fr(0.0));
    assert_eq!(GridTrack::percent(4.0), GridTrack::percent(1.0));
}

#[test]
fn grid_layout_places_the_gpui_holy_grail_in_one_pass() {
    let root = div().grid().grid_cols(5).grid_rows(5).size(500.0, 500.0);
    let children = [
        div().row_span(1).col_span_full(),
        div().col_span(1).row_span(3),
        div().col_span(3).row_span(3),
        div().col_span(1).row_span(3),
        div().row_span(1).col_span_full(),
    ];
    let mut taffy = TaffyTree::<()>::new();
    let child_nodes = children
        .iter()
        .map(|child| taffy.new_leaf(child.layout.clone()).unwrap())
        .collect::<Vec<_>>();
    let root_node = taffy.new_with_children(root.layout, &child_nodes).unwrap();
    taffy
        .compute_layout(
            root_node,
            TaffySize {
                width: AvailableSpace::Definite(500.0),
                height: AvailableSpace::Definite(500.0),
            },
        )
        .unwrap();

    let expected = [
        (0.0, 0.0, 500.0, 100.0),
        (0.0, 100.0, 100.0, 300.0),
        (100.0, 100.0, 300.0, 300.0),
        (400.0, 100.0, 100.0, 300.0),
        (0.0, 400.0, 500.0, 100.0),
    ];
    for (node, (x, y, width, height)) in child_nodes.into_iter().zip(expected) {
        let layout = taffy.layout(node).unwrap();
        assert_eq!((layout.location.x, layout.location.y), (x, y));
        assert_eq!((layout.size.width, layout.size.height), (width, height));
    }
}

#[test]
fn string_children_become_text_nodes() {
    let element = div().child("hello").child(String::from("world"));
    assert_eq!(element.children.len(), 2);
    assert!(matches!(&element.children[0].kind, ElementKind::Text(value) if &**value == "hello"));
}

#[test]
fn styled_text_is_a_single_inherited_text_leaf() {
    let content = StyledText::new("hello world")
        .with_highlights([(6..11, crate::HighlightStyle::default().font_bold())]);
    let element = div().text_lg().child(content);

    assert_eq!(element.children.len(), 1);
    let ElementKind::StyledText(styled) = &element.children[0].kind else {
        panic!("styled text should remain one leaf");
    };
    assert_eq!(&**styled.content(), "hello world");
    assert_eq!(styled.highlights().len(), 1);
}

#[test]
fn svg_elements_retain_asset_fit_and_render_transform() {
    let asset = Svg::from_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10"/></svg>"#,
    )
    .unwrap();
    let transform = SvgTransform::new().scale(1.5).translate(2.0, 3.0);
    let element = svg(&asset)
        .object_fit(ObjectFit::Cover)
        .svg_transform(transform);
    let ElementKind::Svg(svg) = element.kind else {
        panic!("expected an SVG element");
    };
    assert_eq!(svg.svg, asset);
    assert_eq!(svg.object_fit, ObjectFit::Cover);
    assert_eq!(svg.transform, transform);
}

#[test]
fn path_elements_retain_fit_and_optional_background() {
    let mut builder = crate::PathBuilder::fill();
    builder.move_to(crate::Point::new(0.0, 0.0));
    builder.line_to(crate::Point::new(20.0, 0.0));
    builder.line_to(crate::Point::new(10.0, 10.0));
    builder.close();
    let asset = builder.build().unwrap();
    let color = Color::rgb8(14, 165, 233);
    let element = path(&asset)
        .object_fit(ObjectFit::Cover)
        .path_background(color);
    let ElementKind::Path(path) = element.kind else {
        panic!("expected a path element");
    };
    assert_eq!(path.path, asset);
    assert_eq!(path.object_fit, ObjectFit::Cover);
    assert_eq!(path.background, Some(Background::Solid(color)));
}

#[test]
fn canvas_uses_web_default_dimensions() {
    let element = canvas(|bounds, context| {
        assert_eq!(bounds, context.bounds());
    });
    assert_eq!(element.layout.size.width, Dimension::length(300.0));
    assert_eq!(element.layout.size.height, Dimension::length(150.0));
    assert!(matches!(element.kind, ElementKind::Canvas(_)));
}

#[test]
fn custom_shader_elements_retain_assets_and_sanitized_parameters() {
    let shader = CustomShader::new(
        r#"
fn quickgui_fragment(input: QuickGuiShaderInput) -> vec4<f32> {
return vec4<f32>(input.uv, input.params[0].x, 1.0);
}
"#,
    )
    .unwrap();
    let element =
        custom_shader(shader.clone()).shader_parameters(ShaderParameters::new().float(0, f32::NAN));
    let ElementKind::CustomShader(element_shader) = element.kind else {
        panic!("expected a custom shader element");
    };

    assert_eq!(element_shader.shader, shader);
    assert_eq!(element_shader.parameters.vectors()[0][0], 0.0);
}

#[test]
fn named_ids_are_stable() {
    assert_eq!(ElementId::named("save"), ElementId::named("save"));
    assert_ne!(ElementId::named("save"), ElementId::named("cancel"));
}

#[test]
fn button_is_semantic_and_focusable() {
    let element = button().focus(|style| style.border(2.0, Color::WHITE));
    assert_eq!(element.accessibility.role, AccessibilityRole::Button);
    assert!(element.focusable);
    assert_eq!(element.focus.border_width, Some(2.0));
}

#[test]
fn click_handlers_upgrade_plain_divs_to_buttons() {
    let element = div().clickable();
    assert_eq!(element.accessibility.role, AccessibilityRole::Button);
    assert!(element.focusable);
}

#[test]
fn app_region_builders_match_web_drag_and_no_drag_values() {
    assert_eq!(div().app_region_drag().app_region, Some(AppRegion::Drag));
    assert_eq!(
        button().app_region_no_drag().app_region,
        Some(AppRegion::NoDrag)
    );
}

#[test]
fn hidden_matches_display_none_and_display_helpers_restore_it() {
    let hidden = div().flex().hidden();
    assert!(hidden.is_display_none());
    assert_eq!(hidden.layout.display, Display::None);
    assert_eq!(hidden.clone().block().layout.display, Display::Block);
    assert_eq!(hidden.clone().flex().layout.display, Display::Flex);
    assert_eq!(hidden.grid().layout.display, Display::Grid);

    let invisible = div().grid().invisible();
    assert!(invisible.is_visibility_hidden());
    assert_eq!(invisible.layout.display, Display::Grid);
    assert_eq!(invisible.clone().visible().visibility, Visibility::Visible);
    assert_eq!(invisible.visibility, Visibility::Hidden);
}

#[test]
fn text_alignment_helpers_match_gpui_and_inherit() {
    assert_eq!(div().text_left().typography.align, Some(TextAlign::Left));
    assert_eq!(
        div().text_center().typography.align,
        Some(TextAlign::Center)
    );
    assert_eq!(div().text_right().typography.align, Some(TextAlign::Right));
    assert_eq!(
        div().text_justify().typography.align,
        Some(TextAlign::Justify)
    );

    let inherited = TextStyle::new(14.0, Color::WHITE).align(TextAlign::Right);
    assert_eq!(
        TypographyStyle::default().resolve(&inherited).align,
        TextAlign::Right
    );
    assert_eq!(
        div().text_center().typography.resolve(&inherited).align,
        TextAlign::Center
    );
}

#[test]
fn font_configuration_is_inherited_and_complete_fonts_can_clear_fallbacks() {
    let inherited_features =
        FontFeatures::new().disable(crate::FontFeatureTag::CONTEXTUAL_ALTERNATES);
    let inherited_fallbacks = FontFallbacks::from_fonts(["Apple Color Emoji"]);
    let inherited = TextStyle::new(14.0, Color::WHITE)
        .family(FontFamily::Monospace)
        .font_features(inherited_features.clone())
        .font_fallbacks(inherited_fallbacks.clone())
        .weight(Weight::SEMIBOLD)
        .font_style(GlyphStyle::Italic)
        .font_thicken(true);

    let resolved = TypographyStyle::default().resolve(&inherited);
    assert_eq!(resolved.features, inherited_features);
    assert_eq!(resolved.fallbacks, Some(inherited_fallbacks));
    assert!(resolved.font_thicken);

    let local_features = FontFeatures::new().enable(crate::FontFeatureTag::SLASHED_ZERO);
    let resolved = div()
        .font_features(local_features.clone())
        .font_fallbacks(FontFallbacks::new())
        .font_thicken(false)
        .typography
        .resolve(&inherited);
    assert_eq!(resolved.family, FontFamily::Monospace);
    assert_eq!(resolved.features, local_features);
    assert_eq!(resolved.fallbacks, None);
    assert_eq!(resolved.weight, Weight::SEMIBOLD);
    assert!(!resolved.font_thicken);

    let resolved = div()
        .font(
            Font::new("Inter")
                .features(FontFeatures::new().enable(crate::FontFeatureTag::TABULAR_NUMBERS))
                .bold(),
        )
        .typography
        .resolve(&inherited);
    assert_eq!(resolved.family, FontFamily::named("Inter"));
    assert_eq!(
        resolved
            .features
            .value(crate::FontFeatureTag::TABULAR_NUMBERS),
        Some(1)
    );
    assert_eq!(resolved.fallbacks, None);
    assert_eq!(resolved.weight, Weight::BOLD);
    assert_eq!(resolved.font_style, GlyphStyle::Normal);
}

#[test]
fn font_style_and_text_decorations_inherit_and_can_be_reset() {
    let accent = Color::rgb8(56, 189, 248);
    let inherited = TextStyle::new(14.0, Color::WHITE)
        .font_style(GlyphStyle::Italic)
        .underline_color(accent)
        .strikethrough();
    let resolved = TypographyStyle::default().resolve(&inherited);
    assert_eq!(resolved.font_style, GlyphStyle::Italic);
    assert_eq!(resolved.underline, TextUnderline::Single);
    assert_eq!(resolved.underline_color, Some(accent));
    assert!(resolved.strikethrough);

    let reset = div()
        .not_italic()
        .text_decoration_none()
        .typography
        .resolve(&inherited);
    assert_eq!(reset.font_style, GlyphStyle::Normal);
    assert_eq!(reset.underline, TextUnderline::None);
    assert_eq!(reset.underline_color, None);
    assert!(!reset.underline_wavy);
    assert_eq!(reset.underline_thickness, 1.0);
    assert!(!reset.strikethrough);
    assert_eq!(reset.strikethrough_color, None);

    let decorated = div()
        .italic()
        .double_underline()
        .text_decoration_color(accent)
        .line_through();
    assert_eq!(decorated.typography.font_style, Some(GlyphStyle::Italic));
    assert_eq!(decorated.typography.underline, Some(TextUnderline::Double));
    assert_eq!(decorated.typography.underline_color, Some(Some(accent)));
    assert_eq!(decorated.typography.underline_wavy, Some(false));
    assert_eq!(decorated.typography.underline_thickness, Some(1.0));
    assert_eq!(decorated.typography.strikethrough, Some(true));
}

#[test]
fn advanced_text_decoration_helpers_match_gpui_and_inherit() {
    let inherited = TextStyle::new(14.0, Color::WHITE)
        .underline()
        .text_decoration_8()
        .text_decoration_wavy();
    let resolved = TypographyStyle::default().resolve(&inherited);
    assert_eq!(resolved.underline, TextUnderline::Single);
    assert!(resolved.underline_wavy);
    assert_eq!(resolved.underline_thickness, 8.0);

    let local = div().underline().text_decoration_4().text_decoration_wavy();
    assert_eq!(local.typography.underline, Some(TextUnderline::Single));
    assert_eq!(local.typography.underline_wavy, Some(true));
    assert_eq!(local.typography.underline_thickness, Some(4.0));
    let solid = local.text_decoration_solid().typography.resolve(&inherited);
    assert!(!solid.underline_wavy);
    assert_eq!(solid.underline_thickness, 4.0);

    for (element, thickness) in [
        (div().text_decoration_0(), 0.0),
        (div().text_decoration_1(), 1.0),
        (div().text_decoration_2(), 2.0),
        (div().text_decoration_4(), 4.0),
        (div().text_decoration_8(), 8.0),
    ] {
        assert_eq!(element.typography.underline, Some(TextUnderline::Single));
        assert_eq!(element.typography.underline_thickness, Some(thickness));
    }
}

#[test]
fn text_overflow_helpers_match_gpui_and_inherit() {
    assert_eq!(
        div().whitespace_nowrap().typography.wrap,
        Some(TextWrap::None)
    );
    assert_eq!(
        div().whitespace_normal().typography.wrap,
        Some(TextWrap::Word)
    );
    assert!(matches!(
        div().text_ellipsis().typography.text_overflow,
        Some(TextOverflow::Truncate(affix)) if affix.as_ref() == "…"
    ));
    assert!(matches!(
        div().text_ellipsis_start().typography.text_overflow,
        Some(TextOverflow::TruncateStart(affix)) if affix.as_ref() == "…"
    ));
    assert!(matches!(
        div().text_ellipsis_middle().typography.text_overflow,
        Some(TextOverflow::TruncateMiddle(affix)) if affix.as_ref() == "…"
    ));

    let truncated = div().truncate();
    assert_eq!(truncated.typography.wrap, Some(TextWrap::None));
    assert!(matches!(
        truncated.typography.text_overflow,
        Some(TextOverflow::Truncate(_))
    ));
    assert_eq!(truncated.layout.overflow.x, Overflow::Hidden);
    assert_eq!(truncated.layout.overflow.y, Overflow::Hidden);

    let clamped = div().line_clamp(0);
    assert_eq!(clamped.typography.line_clamp, Some(1));
    assert_eq!(clamped.layout.overflow.x, Overflow::Hidden);

    let inherited = TextStyle::new(14.0, Color::WHITE)
        .white_space(WhiteSpace::Nowrap)
        .text_overflow(TextOverflow::ellipsis_start())
        .line_clamp(3);
    let resolved = TypographyStyle::default().resolve(&inherited);
    assert_eq!(resolved.wrap, TextWrap::None);
    assert!(matches!(
        resolved.text_overflow,
        Some(TextOverflow::TruncateStart(_))
    ));
    assert_eq!(resolved.line_clamp, Some(3));

    let overridden = div()
        .whitespace_normal()
        .text_ellipsis_middle()
        .line_clamp(2)
        .typography
        .resolve(&inherited);
    assert_eq!(overridden.wrap, TextWrap::Word);
    assert!(matches!(
        overridden.text_overflow,
        Some(TextOverflow::TruncateMiddle(_))
    ));
    assert_eq!(overridden.line_clamp, Some(2));
}

#[test]
fn virtual_scroll_binds_the_list_offset_and_clips_the_viewport() {
    let mut list = VirtualList::new(100, 10.0);
    list.set_viewport_height(100.0);
    list.scroll_to(240.0);
    let element = div().virtual_scroll(&list);
    let scroll = element.virtual_scroll.as_ref().expect("virtual scroll");

    assert_eq!(element.layout.overflow.y, Overflow::Hidden);
    assert_eq!(scroll.max_offset_y, 900.0);
    assert_eq!(scroll.handle.offset(), 240.0);
    list.scroll_to(500.0);
    assert_eq!(scroll.handle.offset(), 500.0);
}

#[test]
fn variable_virtual_scroll_binds_sparse_state_and_measurement_revision() {
    let list = ListState::new(100, 24.0);
    list.set_viewport_size(300.0, 120.0);
    list.scroll_to(crate::ListOffset {
        item_ix: 10,
        offset_in_item: 4.0,
    });
    let element = div().variable_virtual_scroll(&list);
    let scroll = element.virtual_scroll.as_ref().expect("virtual scroll");

    assert_eq!(element.layout.overflow.y, Overflow::Hidden);
    assert_eq!(scroll.handle.offset(), 244.0);
    assert_eq!(scroll.max_offset_y, 2_280.0);
    assert_eq!(
        scroll.measurement_revision,
        scroll.handle.measurement_revision()
    );
}

#[test]
fn text_inputs_have_native_semantics_and_single_line_defaults() {
    let element = text_input("hello").placeholder("Type here");
    assert_eq!(element.accessibility.role, AccessibilityRole::TextInput);
    assert!(element.focusable);
    assert_eq!(element.cursor_style, Some(CursorStyle::IBeam));
    assert!(!element.cursor_style_explicit);
    assert_eq!(element.typography.wrap, Some(TextWrap::None));
    assert!(matches!(
        &element.kind,
        ElementKind::TextInput(input)
            if input.value.as_ref() == "hello"
                && input.placeholder.as_ref() == "Type here"
                && !input.multiline
                && !input.password
    ));
}

#[test]
fn password_inputs_expose_secure_semantics_without_replacing_the_value() {
    let element = text_input("sk-secret").password(true);

    assert_eq!(element.accessibility.role, AccessibilityRole::PasswordInput);
    assert!(matches!(
        &element.kind,
        ElementKind::TextInput(input) if input.password && !input.multiline
    ));

    let revealed = element.password(false);
    assert_eq!(revealed.accessibility.role, AccessibilityRole::TextInput);
    assert!(matches!(
        &revealed.kind,
        ElementKind::TextInput(input) if !input.password && input.value.as_ref() == "sk-secret"
    ));
}

#[test]
fn cursor_helpers_cover_the_gpui_and_tailwind_vocabulary() {
    let cases = [
        (div().cursor_default(), CursorStyle::Arrow),
        (div().cursor_pointer(), CursorStyle::PointingHand),
        (div().cursor_text(), CursorStyle::IBeam),
        (div().cursor_move(), CursorStyle::ClosedHand),
        (div().cursor_not_allowed(), CursorStyle::OperationNotAllowed),
        (div().cursor_context_menu(), CursorStyle::ContextualMenu),
        (div().cursor_crosshair(), CursorStyle::Crosshair),
        (
            div().cursor_vertical_text(),
            CursorStyle::IBeamCursorForVerticalLayout,
        ),
        (div().cursor_alias(), CursorStyle::DragLink),
        (div().cursor_copy(), CursorStyle::DragCopy),
        (div().cursor_no_drop(), CursorStyle::OperationNotAllowed),
        (div().cursor_grab(), CursorStyle::OpenHand),
        (div().cursor_grabbing(), CursorStyle::ClosedHand),
        (div().cursor_ew_resize(), CursorStyle::ResizeLeftRight),
        (div().cursor_ns_resize(), CursorStyle::ResizeUpDown),
        (
            div().cursor_nesw_resize(),
            CursorStyle::ResizeUpRightDownLeft,
        ),
        (
            div().cursor_nwse_resize(),
            CursorStyle::ResizeUpLeftDownRight,
        ),
        (div().cursor_col_resize(), CursorStyle::ResizeColumn),
        (div().cursor_row_resize(), CursorStyle::ResizeRow),
        (div().cursor_n_resize(), CursorStyle::ResizeUp),
        (div().cursor_e_resize(), CursorStyle::ResizeRight),
        (div().cursor_s_resize(), CursorStyle::ResizeDown),
        (div().cursor_w_resize(), CursorStyle::ResizeLeft),
    ];

    for (element, expected) in cases {
        assert_eq!(element.cursor_style, Some(expected));
        assert!(element.cursor_style_explicit);
    }
}

#[test]
fn explicit_cursor_wins_regardless_of_builder_order() {
    let cursor_before_behavior = div().cursor_crosshair().clickable();
    let cursor_after_behavior = div().clickable().cursor_default();
    let automatic_button = button();

    assert_eq!(
        cursor_before_behavior.cursor_style,
        Some(CursorStyle::Crosshair)
    );
    assert!(cursor_before_behavior.cursor_style_explicit);
    assert_eq!(cursor_after_behavior.cursor_style, Some(CursorStyle::Arrow));
    assert!(cursor_after_behavior.cursor_style_explicit);
    assert_eq!(
        automatic_button.cursor_style,
        Some(CursorStyle::PointingHand)
    );
    assert!(!automatic_button.cursor_style_explicit);
}

#[test]
fn interaction_states_accept_the_same_cursor_helpers() {
    let element = div()
        .hover(|style| style.cursor_crosshair())
        .active(|style| style.cursor_grabbing())
        .focus(|style| style.cursor_text())
        .invalid_style(|style| style.cursor_not_allowed())
        .dragging(|style| style.cursor_copy())
        .drag_over(|style| style.cursor_alias());

    assert_eq!(element.hover.cursor_style, Some(CursorStyle::Crosshair));
    assert_eq!(element.active.cursor_style, Some(CursorStyle::ClosedHand));
    assert_eq!(element.focus.cursor_style, Some(CursorStyle::IBeam));
    assert_eq!(
        element.invalid_style.cursor_style,
        Some(CursorStyle::OperationNotAllowed)
    );
    assert_eq!(element.dragging.cursor_style, Some(CursorStyle::DragCopy));
    assert_eq!(element.drag_over.cursor_style, Some(CursorStyle::DragLink));
    assert!(!element.has_stateful_paint());
    assert!(element.has_stateful_cursor());
}

#[test]
fn groups_and_group_state_variants_are_declared_separately() {
    let group = div().group();
    assert!(group.group);
    // A group tracks state for its members; it paints nothing stateful of its own.
    assert!(!group.has_stateful_paint());

    let member = div().group_hover(|style| {
        style
            .bg(Color::BLACK)
            .border_width(2.0)
            .outline(1.0, Color::WHITE)
            .outline_dashed()
    });
    assert!(!member.group);
    let [entry] = member.group_styles.as_slice() else {
        panic!("one group style is declared");
    };
    assert_eq!(entry.state, GroupState::Hover);
    assert!(entry.target.is_none());
    assert_eq!(entry.style.background, Some(Color::BLACK));
    assert_eq!(entry.style.border_width, Some(2.0));
    assert_eq!(
        entry.style.outline.map(|outline| outline.style),
        Some(BorderStyle::Dashed)
    );
    // A member paints on its group's behalf, so its own hit region stays inert.
    assert!(!member.has_stateful_paint());

    let named = div().group_named("sidebar");
    assert!(named.group);
    assert_eq!(named.group_name.as_deref(), Some("sidebar"));
    let follower = div()
        .group_hover_named("sidebar", |style| style.opacity(1.0))
        .group_active(|style| style.opacity(0.8));
    assert_eq!(follower.group_styles.len(), 2);
    assert_eq!(follower.group_styles[0].target.as_deref(), Some("sidebar"));
    assert_eq!(follower.group_styles[0].style.opacity, Some(1.0));
    assert_eq!(follower.group_styles[1].state, GroupState::Active);
    assert!(follower.group_styles[1].target.is_none());
    assert!(
        std::panic::catch_unwind(|| {
            div().group_named("x".repeat(MAX_HOVER_GROUP_NAME_BYTES + 1))
        })
        .is_err()
    );
    assert!(std::panic::catch_unwind(|| div().group_hover_named("", |style| style)).is_err());
    assert!(
        std::panic::catch_unwind(|| {
            (0..=MAX_GROUP_STYLES_PER_ELEMENT)
                .fold(div(), |element, _| element.group_hover(|style| style))
        })
        .is_err()
    );

    let within = div().focus_within(|style| style.bg(Color::WHITE));
    assert_eq!(within.focus_within.background, Some(Color::WHITE));
    assert!(!within.has_stateful_paint());

    let mut overlaid = ElementStateStyle::default().bg(Color::BLACK).rounded(4.0);
    overlaid.overlay(&ElementStateStyle::default().bg(Color::WHITE).opacity(0.5));
    assert_eq!(overlaid.background, Some(Color::WHITE));
    assert_eq!(overlaid.radius, Some(4.0));
    assert_eq!(overlaid.opacity, Some(0.5));

    let dotted = ElementStateStyle::default().outline_dotted();
    let outline = dotted.outline.expect("a dotted outline is declared");
    assert_eq!((outline.width, outline.style), (0.0, BorderStyle::Dotted));
    assert_eq!(
        ElementStateStyle::default().border_width(-4.0).border_width,
        Some(0.0)
    );
}

#[test]
fn opacity_is_bounded_and_interaction_state_opacity_is_paint_only() {
    assert_eq!(div().opacity(-1.0).visual.opacity, 0.0);
    assert_eq!(div().opacity(2.0).visual.opacity, 1.0);
    assert_eq!(div().opacity(f32::NAN).visual.opacity, 1.0);

    let element = div().hover(|style| style.opacity(0.35));
    assert_eq!(element.hover.opacity, Some(0.35));
    assert!(element.has_stateful_paint());
    assert!(!element.has_stateful_cursor());
}

#[test]
fn hit_slop_is_nonnegative_and_does_not_enter_layout_style() {
    let element = div().hit_slop(Insets {
        top: 4.0,
        right: -2.0,
        bottom: f32::NAN,
        left: 7.0,
    });

    assert_eq!(
        element.hit_slop,
        Insets {
            top: 4.0,
            right: 0.0,
            bottom: 0.0,
            left: 7.0,
        }
    );
    assert_eq!(element.layout, Style::default());
}

#[test]
fn previous_focus_restoration_is_a_retained_behavior_marker() {
    let element = div().restore_previous_focus();

    assert!(element.restore_previous_focus);
    assert_eq!(element.layout, Style::default());
}

#[test]
fn styled_text_areas_keep_one_bounded_controlled_run_table() {
    let element = styled_text_area(
        crate::styled_text("let answer = 42").with_highlights([(
            0..3,
            crate::HighlightStyle::default()
                .font_bold()
                .color(Color::rgb8(196, 181, 253)),
        )]),
    );
    let ElementKind::TextInput(input) = &element.kind else {
        panic!("expected attributed text area");
    };

    assert!(input.multiline);
    assert_eq!(input.value.as_ref(), "let answer = 42");
    assert_eq!(input.highlights.len(), 1);
    assert_eq!(input.highlights[0].range(), 0..3);
    assert_eq!(element.typography.wrap, Some(TextWrap::Word));
}

#[test]
fn text_input_constraints_and_invalid_state_are_declarative() {
    let element = text_input("12")
        .max_length(4)
        .input_filter(|value| value.chars().all(|character| character.is_ascii_digit()))
        .invalid(true)
        .validation_message("Digits only")
        .invalid_style(|style| style.border(3.0, Color::rgb8(239, 68, 68)));
    let ElementKind::TextInput(input) = &element.kind else {
        panic!("expected text input");
    };

    assert_eq!(input.constraints.max_length, Some(4));
    assert!(input.constraints.filter.as_ref().unwrap()("1234"));
    assert!(!input.constraints.filter.as_ref().unwrap()("12a"));
    assert!(element.accessibility.invalid);
    assert_eq!(
        element.accessibility.validation_message.as_deref(),
        Some("Digits only")
    );
    assert_eq!(element.invalid_style.border_width, Some(3.0));
}

#[test]
fn tooltips_attach_accessible_descriptions_without_changing_control_semantics() {
    let element = div().id(41_u64).tooltip("Inspect details");

    assert!(element.tooltip.is_some());
    assert_eq!(
        element.accessibility.description.as_deref(),
        Some("Inspect details")
    );
    assert!(!element.clickable);
    assert!(!element.focusable);
}

#[test]
fn point_anchors_sanitize_geometry_and_default_to_zero_gap() {
    let element = overlay().anchor_at(
        crate::Point::new(f32::NAN, f32::INFINITY),
        AnchorPlacement::BottomStart,
    );
    let anchor = element.anchor.expect("point anchor");

    assert_eq!(anchor.target, AnchorTarget::Point(crate::Point::ZERO));
    assert_eq!(anchor.gap, 0.0);
    assert_eq!(anchor.viewport_margin, DEFAULT_VIEWPORT_MARGIN);
}

#[test]
fn anchor_geometry_builders_are_bounded_and_only_apply_to_anchored_elements() {
    let anchored = crate::div()
        .anchor_to("trigger", AnchorPlacement::TopEnd)
        .anchor_gap(-4.0)
        .anchor_align_offset(f32::NAN)
        .anchor_sticky(false)
        .viewport_margin(-1.0);
    let anchor = anchored.anchor.expect("element anchor");
    assert_eq!(anchor.gap, 0.0);
    assert_eq!(anchor.align_offset, 0.0);
    assert_eq!(anchor.viewport_margin, 0.0);
    assert!(!anchor.sticky);

    let defaults = crate::div().anchor_to("trigger", AnchorPlacement::TopEnd);
    let anchor = defaults.anchor.expect("element anchor");
    assert_eq!(anchor.align_offset, 0.0);
    assert!(anchor.sticky);

    // The builders are inert on an element that declares no anchor at all.
    let plain = crate::div().anchor_align_offset(12.0).anchor_sticky(false);
    assert!(plain.anchor.is_none());
}

#[test]
fn anchor_placement_handles_publish_only_real_changes() {
    let handle = crate::AnchorPlacementHandle::new();
    assert_eq!(handle.resolved(), None);
    assert_eq!(
        handle.placement_or(AnchorPlacement::LeftEnd),
        AnchorPlacement::LeftEnd
    );
    assert_eq!(handle.revision(), 0);
    assert!(format!("{handle:?}").contains("AnchorPlacementHandle"));

    let resolved = crate::ResolvedAnchorPlacement {
        placement: AnchorPlacement::TopStart,
        anchor: crate::Rect::new(1.0, 2.0, 3.0, 4.0),
        bounds: crate::Rect::new(5.0, 6.0, 7.0, 8.0),
        available: crate::Size::new(9.0, 10.0),
        anchor_hidden: false,
    };
    let bound = handle.clone();
    bound.report(resolved);
    assert_eq!(handle.resolved(), Some(resolved));
    assert_eq!(handle.revision(), 1);
    assert_eq!(
        handle.placement_or(AnchorPlacement::LeftEnd),
        AnchorPlacement::TopStart
    );
    assert_eq!(resolved.side(), crate::AnchorSide::Top);
    assert_eq!(resolved.align(), crate::AnchorAlign::Start);

    // An unchanged placement never bumps the revision, so it requests no correcting frame.
    handle.report(resolved);
    assert_eq!(handle.revision(), 1);
    handle.report(crate::ResolvedAnchorPlacement {
        anchor_hidden: true,
        ..resolved
    });
    assert_eq!(handle.revision(), 2);

    handle.clear();
    assert_eq!(handle.resolved(), None);
    assert_eq!(handle.revision(), 3);
    handle.clear();
    assert_eq!(handle.revision(), 3);
    assert_eq!(handle, bound);
    assert_ne!(handle, crate::AnchorPlacementHandle::new());
}

#[test]
fn anchor_sides_and_alignments_round_trip_through_placements() {
    let placements = [
        AnchorPlacement::TopStart,
        AnchorPlacement::Top,
        AnchorPlacement::TopEnd,
        AnchorPlacement::BottomStart,
        AnchorPlacement::Bottom,
        AnchorPlacement::BottomEnd,
        AnchorPlacement::LeftStart,
        AnchorPlacement::Left,
        AnchorPlacement::LeftEnd,
        AnchorPlacement::RightStart,
        AnchorPlacement::Right,
        AnchorPlacement::RightEnd,
    ];
    for placement in placements {
        let side = crate::AnchorSide::of(placement);
        let align = crate::AnchorAlign::of(placement);
        assert_eq!(crate::anchor_placement(side, align), placement);
        assert_eq!(side.opposite().opposite(), side);
        assert_ne!(side.opposite(), side);
        assert_eq!(
            side.is_vertical(),
            matches!(side, crate::AnchorSide::Top | crate::AnchorSide::Bottom)
        );
    }
}

#[test]
fn text_areas_have_multiline_semantics_and_wrapping_defaults() {
    let element = text_area("one\ntwo").placeholder("Notes");
    assert_eq!(
        element.accessibility.role,
        AccessibilityRole::MultilineTextInput
    );
    assert_eq!(element.typography.wrap, Some(TextWrap::Word));
    assert_eq!(element.layout.size.width, Dimension::length(320.0));
    assert_eq!(element.layout.size.height, Dimension::length(160.0));
    assert!(matches!(
        &element.kind,
        ElementKind::TextInput(input)
            if input.value.as_ref() == "one\ntwo" && input.multiline
    ));
}

#[test]
fn shadow_utilities_are_paint_only_and_state_styles_can_remove_them() {
    let element = div().shadow_md().hover(|style| style.shadow_none());
    let shadows = element.visual.shadows.as_deref().unwrap();
    assert_eq!(shadows.len(), 2);
    assert_eq!(shadows[0].offset().y, 4.0);
    assert_eq!(element.layout, Style::default());
    assert!(
        element
            .hover
            .shadows
            .as_deref()
            .is_some_and(<[BoxShadow]>::is_empty)
    );

    let hover_elevation = ElementStateStyle::default().shadow_lg();
    assert_eq!(hover_elevation.shadows.as_deref().unwrap().len(), 2);
}

#[test]
fn box_shadow_lists_fail_at_the_fixed_retention_bound() {
    let shadows = || {
        (0..=MAX_BOX_SHADOWS_PER_ELEMENT)
            .map(|index| BoxShadow::new(0.0, index as f32, Color::BLACK))
    };
    assert!(std::panic::catch_unwind(|| div().shadows(shadows())).is_err());
    assert!(std::panic::catch_unwind(|| ElementStateStyle::default().shadows(shadows())).is_err());
}

#[test]
fn background_gradients_replace_the_solid_fill_and_stay_paint_only() {
    let element = div().bg(Color::WHITE).bg_linear_gradient(
        crate::GradientDirection::ToRight,
        [Color::BLACK, Color::WHITE],
    );
    assert!(element.visual.background_gradient.is_some());
    assert_eq!(element.layout, div().layout);

    // A later solid background clears the gradient again.
    let solid = element.bg(Color::WHITE);
    assert!(solid.visual.background_gradient.is_none());
    assert_eq!(solid.visual.background, Some(Color::WHITE));

    // Passing a solid color through the gradient entry point stays solid.
    let plain = div().bg_gradient(Color::BLACK);
    assert!(plain.visual.background_gradient.is_none());
    assert_eq!(plain.visual.background, Some(Color::BLACK));

    let hovered = div().hover(|style| {
        style.bg_gradient(crate::Gradient::conic(0.0, [Color::BLACK, Color::WHITE]))
    });
    assert!(hovered.hover.background_gradient.is_some());
    assert!(hovered.has_stateful_paint());
}

#[test]
fn per_corner_radii_are_bounded_and_replace_the_uniform_radius() {
    let element = div().rounded(8.0).rounded_tl(20.0).rounded_br(2.0);
    let corners = element
        .visual
        .corner_radii
        .expect("per-corner radii are set");
    assert_eq!(corners, Corners::new(20.0, 8.0, 2.0, 8.0));

    // A later uniform radius clears the per-corner override.
    let uniform = element.rounded(4.0);
    assert!(uniform.visual.corner_radii.is_none());
    assert_eq!(uniform.visual.radius, 4.0);

    assert_eq!(
        div().rounded_t(6.0).visual.corner_radii,
        Some(Corners::new(6.0, 6.0, 0.0, 0.0))
    );
    assert_eq!(
        div().rounded_r(6.0).visual.corner_radii,
        Some(Corners::new(0.0, 6.0, 6.0, 0.0))
    );
    assert_eq!(
        div()
            .corner_radii(Corners::all(f32::INFINITY))
            .visual
            .corner_radii,
        Some(Corners::ZERO)
    );
    assert_eq!(div().rounded(-4.0).visual.radius, 0.0);
    assert_eq!(div().rounded_full().visual.radius, crate::MAX_CORNER_RADIUS);
    // Radii are paint-only: no Taffy style changes.
    assert_eq!(div().rounded_bl(12.0).layout, div().layout);
}

#[test]
fn border_styles_and_outlines_are_bounded_and_never_enter_layout() {
    let dashed = div().border(2.0, Color::WHITE).border_dashed();
    assert_eq!(dashed.visual.border_style, crate::BorderStyle::Dashed);
    assert_eq!(
        div().border_dotted().visual.border_style,
        crate::BorderStyle::Dotted
    );
    assert_eq!(
        dashed.border_solid().visual.border_style,
        crate::BorderStyle::Solid
    );

    let outlined = div()
        .outline(4.0, Color::WHITE)
        .outline_offset(3.0)
        .outline_dashed();
    let outline = outlined.visual.outline.expect("an outline is set");
    assert_eq!(outline.width, 4.0);
    assert_eq!(outline.offset, 3.0);
    assert_eq!(outline.style, crate::BorderStyle::Dashed);
    // The outline is not a Taffy border and does not move layout.
    assert_eq!(outlined.layout, div().layout);
    assert!(outlined.visual.border_widths == Insets::default());
    assert!(outlined.outline_none().visual.outline.is_none());

    assert_eq!(
        div()
            .outline(f32::INFINITY, Color::WHITE)
            .visual
            .outline
            .unwrap()
            .width,
        0.0
    );
    assert_eq!(
        div()
            .outline(1.0, Color::WHITE)
            .outline_offset(-100_000.0)
            .visual
            .outline
            .unwrap()
            .offset,
        -crate::MAX_OUTLINE_OFFSET
    );

    // A ring is only produced when it can actually paint.
    let bounds = Rect::new(10.0, 10.0, 20.0, 20.0);
    let ring = crate::Outline::new(2.0, Color::WHITE).offset(2.0);
    assert_eq!(ring.ring(bounds), Some(Rect::new(6.0, 6.0, 28.0, 28.0)));
    assert!(
        crate::Outline::new(0.0, Color::WHITE)
            .ring(bounds)
            .is_none()
    );
    assert!(
        crate::Outline::new(2.0, Color::TRANSPARENT)
            .ring(bounds)
            .is_none()
    );
}

#[test]
fn background_images_resolve_every_sizing_mode_without_touching_layout() {
    let image = crate::Image::from_rgba(4, 2, vec![255_u8; 4 * 2 * 4]).unwrap();
    let bounds = Rect::new(0.0, 0.0, 40.0, 40.0);

    let auto = crate::BackgroundImage::new(image.clone());
    assert_eq!(auto.tile_size(bounds).unwrap(), crate::Size::new(4.0, 2.0));

    let mut cover = crate::BackgroundImage::new(image.clone());
    cover.size = crate::BackgroundSize::Cover;
    assert_eq!(
        cover.tile_size(bounds).unwrap(),
        crate::Size::new(80.0, 40.0)
    );

    let mut contain = crate::BackgroundImage::new(image.clone());
    contain.size = crate::BackgroundSize::Contain;
    assert_eq!(
        contain.tile_size(bounds).unwrap(),
        crate::Size::new(40.0, 20.0)
    );

    let mut fixed = crate::BackgroundImage::new(image.clone());
    fixed.size = crate::BackgroundSize::Fixed(f32::NAN, 5.0);
    assert!(fixed.tile_size(bounds).is_none());

    let element = div().bg_image(
        image,
        crate::BackgroundSize::Contain,
        crate::BackgroundRepeat::RepeatX,
        crate::BackgroundPosition::new(4.0, f32::NAN),
    );
    let background = element
        .visual
        .background_image
        .as_deref()
        .expect("a background image is set");
    assert_eq!(background.repeat, crate::BackgroundRepeat::RepeatX);
    assert!(background.repeat.repeats_x() && !background.repeat.repeats_y());
    assert_eq!(
        background.position,
        crate::BackgroundPosition::new(1.0, 0.5)
    );
    assert_eq!(element.layout, div().layout);
    assert!(element.bg_image_none().visual.background_image.is_none());
}
