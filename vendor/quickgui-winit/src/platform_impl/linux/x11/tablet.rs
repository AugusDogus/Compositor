//! Tablet valuators on the same XI2 connection as mouse input, avoiding duplicates.
use super::{ffi, Device, XConnection};
use crate::dpi::PhysicalPosition;
use crate::event::{ElementState, TabletEvent};
use x11rb::protocol::xproto::ConnectionExt;

#[derive(Debug)]
struct Axis {
    number: i32,
    min: f64,
    max: f64,
}
#[derive(Debug)]
pub(super) struct Tool {
    pub(super) window: Option<u32>,
    pressure: Axis,
    tilt: [Option<Axis>; 2],
    frame: TabletEvent,
}
impl Tool {
    pub(super) fn new(
        info: &ffi::XIDeviceInfo,
        connection: &XConnection,
        name: &str,
    ) -> Option<Self> {
        if !Device::physical_device(info)
            || Device::classes(info)
                .iter()
                .any(|class| unsafe { (**class)._type == ffi::XITouchClass })
        {
            return None;
        }
        let axis = |label: &[u8]| -> Option<Axis> {
            let atom = connection
                .xcb_connection()
                .intern_atom(false, label)
                .ok()?
                .reply()
                .ok()?
                .atom;
            Device::classes(info).iter().find_map(|class| {
                if unsafe { (**class)._type } != ffi::XIValuatorClass {
                    return None;
                }
                // XIValuatorClass is the protocol discriminator for this class layout.
                let axis = unsafe { &*((*class).cast::<ffi::XIValuatorClassInfo>()) };
                (axis.label == atom as _
                    && axis.max > axis.min
                    && axis.min.is_finite()
                    && axis.max.is_finite())
                .then_some(Axis {
                    number: axis.number,
                    min: axis.min,
                    max: axis.max,
                })
            })
        };
        Some(Self {
            window: None,
            pressure: axis(b"Abs Pressure")?,
            tilt: [axis(b"Abs Tilt X"), axis(b"Abs Tilt Y")],
            frame: TabletEvent {
                position: PhysicalPosition::new(0., 0.),
                pressure: None,
                tilt: None,
                eraser: name.to_ascii_lowercase().contains("eraser"),
                buttons: [false; 3],
                proximity: true,
            },
        })
    }
    pub(super) fn leave(&mut self) -> TabletEvent {
        self.frame.proximity = false;
        self.frame.buttons = [false; 3];
        self.frame
    }
    pub(super) fn update(
        &mut self,
        event: &ffi::XIDeviceEvent,
        button: Option<ElementState>,
    ) -> TabletEvent {
        self.window = Some(event.event as u32);
        self.frame.position = PhysicalPosition::new(event.event_x, event.event_y);
        self.frame.proximity = true;
        // XI2 masks describe the state before a button transition.
        let buttons = if event.buttons.mask_len > 0 && !event.buttons.mask.is_null() {
            unsafe {
                std::slice::from_raw_parts(event.buttons.mask, event.buttons.mask_len as usize)
            }
        } else {
            &[]
        };
        for (index, number) in [1, 3, 2].into_iter().enumerate() {
            self.frame.buttons[index] = buttons
                .first()
                .is_some_and(|mask| mask & (1 << number) != 0);
            if event.detail == number {
                if let Some(state) = button {
                    self.frame.buttons[index] = state == ElementState::Pressed;
                }
            }
        }
        if event.valuators.mask_len > 0
            && !event.valuators.mask.is_null()
            && !event.valuators.values.is_null()
        {
            let mask = unsafe {
                std::slice::from_raw_parts(event.valuators.mask, event.valuators.mask_len as usize)
            };
            let count = mask.iter().map(|byte| byte.count_ones() as usize).sum();
            let values = unsafe { std::slice::from_raw_parts(event.valuators.values, count) };
            if let Some(value) = valuator(mask, values, self.pressure.number) {
                self.frame.pressure = Some(
                    ((value - self.pressure.min) / (self.pressure.max - self.pressure.min))
                        .clamp(0., 1.) as f32,
                );
            }
            for (index, axis) in self.tilt.iter().enumerate() {
                if let Some(axis) = axis {
                    if let Some(value) = valuator(mask, values, axis.number) {
                        let degrees = if value >= 0. {
                            value / axis.max.max(1.)
                        } else {
                            -value / axis.min.min(-1.)
                        } * 90.;
                        self.frame.tilt.get_or_insert([0.; 2])[index] =
                            degrees.clamp(-90., 90.) as f32;
                    }
                }
            }
        }
        self.frame
    }
}
fn valuator(mask: &[u8], values: &[f64], number: i32) -> Option<f64> {
    let number = usize::try_from(number).ok()?;
    let (word, bit) = (number / 8, number % 8);
    if mask.get(word)? & (1 << bit) == 0 {
        return None;
    }
    let index = mask[..word]
        .iter()
        .map(|byte| byte.count_ones() as usize)
        .sum::<usize>()
        + (mask[word] & ((1 << bit) - 1)).count_ones() as usize;
    values.get(index).copied().filter(|value| value.is_finite())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sparse_valuator_indices_and_invalid_values() {
        assert_eq!(valuator(&[0b10100000, 0b10], &[7., 9., 11.], 9), Some(11.));
        assert_eq!(valuator(&[0b10100000], &[7., 9.], 5), Some(7.));
        assert_eq!(valuator(&[0b10100000], &[7., 9.], 6), None);
        assert_eq!(valuator(&[1], &[f64::NAN], 0), None);
        assert_eq!(valuator(&[1], &[], 0), None);
    }
}

#[cfg(test)]
mod packet_tests {
    use super::*;
    #[test]
    fn calibrated_packet_updates_axes_and_tip_before_dispatch_and_releases_on_loss() {
        let mut tool = Tool {
            window: None,
            pressure: Axis {
                number: 2,
                min: 100.,
                max: 1100.,
            },
            tilt: [
                Some(Axis {
                    number: 4,
                    min: -64.,
                    max: 63.,
                }),
                None,
            ],
            frame: TabletEvent {
                position: PhysicalPosition::new(0., 0.),
                pressure: None,
                tilt: None,
                eraser: true,
                buttons: [false; 3],
                proximity: false,
            },
        };
        let mut axis_mask = [0b10100u8];
        let mut values = [350., 31.5];
        let mut buttons = [0u8];
        // XIDeviceEvent is a plain C packet; zero creates empty masks/pointers.
        let mut event: ffi::XIDeviceEvent = unsafe { std::mem::zeroed() };
        event.event = 42;
        event.event_x = 123.;
        event.event_y = 45.;
        event.detail = 1;
        event.valuators.mask_len = 1;
        event.valuators.mask = axis_mask.as_mut_ptr();
        event.valuators.values = values.as_mut_ptr();
        event.buttons.mask_len = 1;
        event.buttons.mask = buttons.as_mut_ptr();
        let sample = tool.update(&event, Some(ElementState::Pressed));
        assert_eq!(sample.pressure, Some(0.25));
        assert_eq!(sample.tilt, Some([45., 0.]));
        assert_eq!(sample.buttons, [true, false, false]);
        assert_eq!(sample.position, PhysicalPosition::new(123., 45.));
        assert!(sample.eraser && sample.proximity);
        let sample = tool.leave();
        assert!(!sample.proximity);
        assert_eq!(sample.buttons, [false; 3]);
    }
}
