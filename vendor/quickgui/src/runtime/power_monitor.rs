use super::*;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub(crate) use windows::WindowsPowerMonitor;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub(crate) use linux::LinuxPowerMonitor;

/// A native operating-system power or session transition.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PowerEvent {
    Suspend,
    Resume,
    LockScreen,
    UnlockScreen,
    /// The login or desktop environment is preparing to shut down or restart the machine.
    ShutdownRequested,
    /// The system changed between AC, battery, or an indeterminate power source.
    PowerSourceChanged(PowerSource),
    /// The operating system reported a new coarse thermal-pressure state.
    ThermalStateChanged(ThermalState),
    /// The operating system enabled or disabled its energy-saving mode.
    LowPowerModeChanged(bool),
    /// The operating system advertised a new CPU speed ceiling in percent.
    CpuSpeedLimitChanged(u8),
}

impl Runtime {
    pub(super) fn invoke_power_event(&mut self, event_loop: &ActiveEventLoop, event: PowerEvent) {
        let Some(mut callback) = self.application_callbacks.power_event.take() else {
            return;
        };
        let mut context = self.event_context();
        callback(event, &mut context);
        self.application_callbacks.power_event = Some(callback);
        self.apply_application_context(event_loop, context);
    }
}
