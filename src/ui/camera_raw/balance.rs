use super::*;
#[derive(Clone, Copy, Default, PartialEq)]
pub(super) enum Work {
    #[default]
    Idle,
    Queued,
    Running,
}
impl Editor {
    pub(super) fn camera_auto_balance(&mut self) {
        if self.camera_raw.balance == Work::Idle && !self.filter_applying() {
            self.camera_raw.balance = Work::Queued;
        }
    }
    pub(in crate::ui) fn start_camera_balance(&mut self, cx: &ViewContext<'_, Self>) {
        if self.camera_raw.balance != Work::Queued {
            return;
        }
        let Ok((document, _)) = self.filter_source() else {
            self.camera_raw.balance = Work::Idle;
            return;
        };
        let Some(source) = document.active_layer().and_then(|l| l.raster()).cloned() else {
            self.camera_raw.balance = Work::Idle;
            return;
        };
        self.camera_raw.balance = Work::Running;
        let token = self.camera_raw.balance_id;
        let color = self.camera_raw.settings.color.clone();
        let task=cx.spawn_background(move||compositor::camera_raw::auto_balance(&source),move|this,result,cx|{
            if this.camera_raw.balance_id!=token{return;}
            this.camera_raw.balance=Work::Idle;
            if !matches!(this.modal,Some(Form::Edit {action:Action::CameraRaw,..})) || this.filter_applying() || this.camera_raw.settings.color!=color {return;}
            let result=result.map_err(|e|compositor::invalid(format!("Automatic white balance worker failed: {e}. Adjust Temperature and Tint manually."))).and_then(|r|r);
            match result {Ok([temperature,tint])=>this.camera_change(|e|{e.settings.color.temperature=temperature;e.settings.color.tint=tint;}),Err(error)=>if let Some(Form::Edit {error:target,..})=&mut this.modal{*target=error.to_string();}}
            this.changed(cx);
        });
        if let Err(error) = task {
            self.camera_raw.balance = Work::Idle;
            if let Some(Form::Edit { error: target, .. }) = &mut self.modal {
                *target = format!(
                    "Could not start automatic white balance: {error}. Adjust Temperature and Tint manually."
                );
            }
        }
    }
}
