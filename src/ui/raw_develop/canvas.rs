use super::*;
use quickgui::{PointerPhase, Rect};

fn placement(d: &Develop, width: f32, height: f32) -> Rect {
    let Some(r) = &d.ready else {
        return Rect::new(0., 0., 1., 1.);
    };
    let side = d.compare == Compare::SideBySide;
    let available = width / if side { 2. } else { 1. };
    let zoom = if d.fit {
        ((available - 32.) / r.preview.width() as f32)
            .min((height - 32.) / r.preview.height() as f32)
            .max(0.001)
    } else {
        d.zoom
    };
    let w = r.preview.width() as f32 * zoom;
    let h = r.preview.height() as f32 * zoom;
    Rect::new(
        (width - w) / 2.
            + if side { available / 2. } else { 0. }
            + if d.fit { 0. } else { d.pan[0] },
        (height - h) / 2. + if d.fit { 0. } else { d.pan[1] },
        w,
        h,
    )
}
fn image_at(image: &DisplayImage, rect: Rect) -> Element {
    image.element(rect)
}

impl Editor {
    pub(super) fn raw_canvas(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let Some(d) = &self.develop else {
            return div();
        };
        let width = (cx.size().width - 292.).max(1.);
        let height = (cx.size().height - 168.).max(1.);
        let rect = placement(d, width, height);
        let mut canvas = div()
            .id("raw-canvas")
            .w(width)
            .h(height)
            .flex_shrink_0()
            .min_h(0.)
            .relative()
            .overflow_hidden()
            .bg(Color::rgb8(22, 24, 28));
        if let Some(r) = &d.ready {
            let shown = if d.clipping { &r.warnings } else { &r.preview };
            match d.compare {
                Compare::Edited => canvas = canvas.child(image_at(shown, rect)),
                Compare::Original => canvas = canvas.child(image_at(&r.before, rect)),
                Compare::SideBySide => {
                    canvas = canvas.child(image_at(shown, rect));
                    canvas = canvas.child(image_at(
                        &r.before,
                        Rect::new(rect.x - width / 2., rect.y, rect.width, rect.height),
                    ));
                }
                Compare::Split => {
                    canvas = canvas.child(image_at(shown, rect));
                    canvas = canvas.child(
                        div()
                            .absolute()
                            .left(rect.x)
                            .top(rect.y)
                            .size(rect.width * d.split, rect.height)
                            .overflow_hidden()
                            .child(image_at(
                                &r.before,
                                Rect::new(0., 0., rect.width, rect.height),
                            )),
                    );
                    canvas = canvas.child(
                        div()
                            .absolute()
                            .left(rect.x + rect.width * d.split)
                            .top(rect.y)
                            .w(2.)
                            .h(rect.height)
                            .bg(Color::WHITE),
                    );
                }
            }
            if d.show_mask
                && let Some(mask) = d
                    .selected_mask
                    .and_then(|i| d.settings.overlays.get(i))
                    .cloned()
            {
                let crop = d.settings.crop;
                canvas = canvas.child(
                    quickgui::canvas(move |_, painter| {
                        let position = |p: raw::Point| {
                            quickgui::Point::new(
                                rect.x + (p.x - crop[0]) / (crop[2] - crop[0]) * rect.width,
                                rect.y + (p.y - crop[1]) / (crop[3] - crop[1]) * rect.height,
                            )
                        };
                        let color = Color::rgb8(255, 188, 75);
                        if mask.kind == raw::OverlayKind::Brush {
                            for point in &mask.points {
                                let p = position(*point);
                                let radius = mask.radius * rect.height / (crop[3] - crop[1]);
                                painter.fill_rounded_rect(
                                    Rect::new(p.x - radius, p.y - radius, radius * 2., radius * 2.),
                                    radius,
                                    color.with_alpha(0.07),
                                );
                            }
                        } else {
                            let a = position(mask.start);
                            let b = position(mask.end);
                            let mut path = quickgui::PathBuilder::stroke(1.5);
                            path.move_to(a);
                            path.line_to(b);
                            if let Ok(path) = path.build() {
                                painter.paint_path(&path, color);
                            }
                            for p in [a, b] {
                                painter.fill_rounded_rect(
                                    Rect::new(p.x - 4., p.y - 4., 8., 8.),
                                    4.,
                                    color,
                                );
                            }
                            if mask.kind == raw::OverlayKind::Radial {
                                let mut path = quickgui::PathBuilder::stroke(1.);
                                for i in 0..=64 {
                                    let angle = i as f32 * std::f32::consts::TAU / 64.;
                                    let p = quickgui::Point::new(
                                        a.x + (b.x - a.x).abs() * angle.cos(),
                                        a.y + (b.y - a.y).abs() * angle.sin(),
                                    );
                                    if i == 0 {
                                        path.move_to(p);
                                    } else {
                                        path.line_to(p);
                                    }
                                }
                                if let Ok(path) = path.build() {
                                    painter.paint_path(&path, color);
                                }
                            }
                        }
                    })
                    .absolute()
                    .left(0.)
                    .top(0.)
                    .size(width, height),
                );
            }
        }
        canvas.on_pointer(cx.pointer_listener("raw-canvas",|this,event,cx|{
            let Some(d)=&mut this.develop else{return;};
            if d.committing||d.ready.is_none()||event.button!=quickgui::MouseButton::Left{return;}
            let rect=placement(d,event.size.width,event.size.height);
            let x=(event.local_position.x-rect.x)/rect.width;let y=(event.local_position.y-rect.y)/rect.height;
            let point=((0. ..=1.).contains(&x)&&(0. ..=1.).contains(&y)).then(||raw::Point::new(d.settings.crop[0]+x*(d.settings.crop[2]-d.settings.crop[0]),d.settings.crop[1]+y*(d.settings.crop[3]-d.settings.crop[1])));
            if event.phase==PointerPhase::Down {d.finish_numeric_input();d.begin_gesture();}
            if d.picker&&event.phase==PointerPhase::Down {
                if let (Some(point),Some(r))=(point,&d.ready) {
                    let p=raw::source_point(point,&d.settings,r.proxy.camera.width() as f32/r.proxy.camera.height() as f32);
                    let wb=raw::sample_white_balance(&r.proxy,p);
                    d.edit(|s|{s.custom_wb=wb;s.white_balance=raw::WhiteBalance::Custom;s.tint=0.;});d.picker=false;
                }
            } else if d.draw_mask {
                if let (Some(point),Some(index))=(point,d.selected_mask) && matches!(event.phase,PointerPhase::Down|PointerPhase::Move) {
                    let start=event.phase==PointerPhase::Down;let last=d.last_brush;
                    d.edit(|s|{
                        let count:usize=s.overlays.iter().map(|m|m.points.len()).sum();
                        let available=8192_usize.saturating_sub(count);
                        if let Some(mask)=s.overlays.get_mut(index) {
                            if mask.kind==raw::OverlayKind::Brush {
                                if available>0 {
                                    if let Some(last)=last.filter(|_|!start) {
                                        let distance=((point.x-last.x).powi(2)+(point.y-last.y).powi(2)).sqrt();
                                        let steps=(distance/(mask.radius*0.3)).ceil().clamp(1.,256.) as usize;
                                        for i in 1..=steps.min(available){let t=i as f32/steps as f32;mask.points.push(raw::Point::new(last.x+(point.x-last.x)*t,last.y+(point.y-last.y)*t));}
                                    }else{mask.points.push(point);}
                                }
                            }else{if start{mask.start=point;}mask.end=point;}
                        }
                    });
                    d.last_brush=Some(point);
                    if d.settings.overlays.iter().map(|m|m.points.len()).sum::<usize>()>=8192 {d.notice="Brush mask point limit reached (8,192). Clear strokes or simplify a mask to continue.".into();}
                }
            }else if event.phase==PointerPhase::Move {
                if d.compare==Compare::Split&&!event.modifiers.contains(Modifiers::ALT){d.split=x.clamp(0.,1.);}
                else{if d.fit {d.zoom=rect.width/d.ready.as_ref().map_or(1,|r|r.preview.width()) as f32;d.fit=false;}d.pan[0]+=event.delta.x;d.pan[1]+=event.delta.y;}
            }
            if matches!(event.phase,PointerPhase::Up|PointerPhase::Cancel) {d.finish_gesture(event.phase==PointerPhase::Cancel);d.last_brush=None;}
            cx.invalidate();
        })).on_scroll_wheel(cx.scroll_wheel_listener("raw-canvas",move |this,event,cx|{
            if let Some(d)=&mut this.develop && !d.committing {
                if d.fit {let rect=placement(d,width,height);d.zoom=rect.width/d.ready.as_ref().map_or(1,|r|r.preview.width()) as f32;d.fit=false;}
                d.zoom=(d.zoom*(event.delta.pixel_delta(24.).y*0.005).exp()).clamp(0.01,16.);
            }cx.invalidate();
        }))
    }
    pub(super) fn raw_curve(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let Some(d) = &self.develop else {
            return div();
        };
        let channel = d.curve_channel;
        let knots = d.settings.curves[channel];
        quickgui::canvas(move |bounds, painter| {
            painter.fill_rect(bounds, Color::rgb8(20, 22, 27));
            for i in 1..4 {
                let p = i as f32 / 4.;
                let mut line = quickgui::PathBuilder::stroke(1.);
                line.move_to(quickgui::Point::new(p * bounds.width, 0.));
                line.line_to(quickgui::Point::new(p * bounds.width, bounds.height));
                line.move_to(quickgui::Point::new(0., p * bounds.height));
                line.line_to(quickgui::Point::new(bounds.width, p * bounds.height));
                if let Ok(path) = line.build() {
                    painter.paint_path(&path, Color::rgb8(55, 58, 63));
                }
            }
            let mut line = quickgui::PathBuilder::stroke(2.);
            for (i, v) in knots.into_iter().enumerate() {
                let p =
                    quickgui::Point::new(i as f32 * bounds.width / 4., (1. - v) * bounds.height);
                if i == 0 {
                    line.move_to(p);
                } else {
                    line.line_to(p);
                }
                painter.fill_rounded_rect(
                    Rect::new(p.x - 4., p.y - 4., 8., 8.),
                    4.,
                    Color::rgb8(90, 165, 255),
                );
            }
            if let Ok(path) = line.build() {
                painter.paint_path(&path, Color::WHITE);
            }
        })
        .id("raw-curve")
        .w(260.)
        .h(180.)
        .flex_shrink_0()
        .on_pointer(cx.pointer_listener("raw-curve", move |this, event, cx| {
            let Some(d) = &mut this.develop else {
                return;
            };
            if d.committing || event.button != quickgui::MouseButton::Left {
                return;
            }
            if event.phase == PointerPhase::Down {
                d.finish_numeric_input();
                d.begin_gesture();
                d.curve_knot = Some(
                    (event.local_position.x / event.size.width * 4.)
                        .round()
                        .clamp(0., 4.) as usize,
                );
            }
            if let Some(knot) = d.curve_knot
                && matches!(event.phase, PointerPhase::Down | PointerPhase::Move)
            {
                let v = (1. - event.local_position.y / event.size.height).clamp(0., 1.);
                d.edit(|s| s.curves[channel][knot] = v);
            }
            if matches!(event.phase, PointerPhase::Up | PointerPhase::Cancel) {
                d.finish_gesture(event.phase == PointerPhase::Cancel);
                d.curve_knot = None;
            }
            cx.invalidate();
        }))
    }
}
