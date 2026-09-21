use super::*;
use quickgui::{PathPromptOptions, SavePathOptions};
#[derive(Clone, Copy)]
pub(super) enum Dialog {
    Export,
    SavePreset,
    LoadPreset,
}
impl Editor {
    pub(super) fn raw_dialog(&mut self, kind: Dialog, cx: &mut EventContext) {
        if let Some(d) = &mut self.develop
            && !d.finish_numeric_input()
        {
            cx.invalidate();
            return;
        }
        let Some(d) = &self.develop else {
            return;
        };
        if d.ready.is_none() || d.committing {
            return;
        }
        let id = d.id;
        let response = if matches!(kind, Dialog::LoadPreset) {
            let options = PathPromptOptions::new()
                .title("Load RAW preset")
                .filters([file_dialogs::file_filter("RAW preset", &["json"])]);
            match cx.prompt_for_paths(options) {
                Ok(response) => {
                    self.await_response(
                        cx,
                        alerts::Operation::Import,
                        response,
                        move |this, result, cx| {
                            if let Some(d) = &mut this.develop
                                && d.id == id
                            {
                                match result {
                                    Ok(Some(paths)) => {
                                        if let Some(path) = paths.into_iter().next() {
                                            d.request = Some(Request::LoadPreset(path));
                                            d.committing = true;
                                        }
                                    }
                                    Ok(None) => {}
                                    Err(e) => {
                                        d.error =
                                            Some(format!("Could not open the preset chooser: {e}"))
                                    }
                                }
                            }
                            cx.invalidate();
                        },
                    );
                    return;
                }
                Err(e) => {
                    if let Some(d) = &mut self.develop {
                        d.error = Some(format!("Could not open the preset chooser: {e}"));
                    }
                    return;
                }
            }
        } else {
            let (title, extension) = if matches!(kind, Dialog::Export) {
                ("Export RAW as 16-bit TIFF", "tif")
            } else {
                ("Save RAW preset", "json")
            };
            let stem = std::path::Path::new(&d.title)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy();
            cx.prompt_for_new_path(
                SavePathOptions::new(
                    std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
                )
                .title(title)
                .suggested_name(format!("{stem}.{extension}"))
                .filters([file_dialogs::file_filter(title, &[extension])]),
            )
        };
        match response {
            Ok(response)=>self.await_response(cx,alerts::Operation::ExportTiff,response,move |this,result,cx| {
                if let Some(d)=&mut this.develop && d.id==id {
                    match result {
                        Ok(Some(path))=>{
                            if matches!(kind,Dialog::SavePreset)&&!path.extension().and_then(|s|s.to_str()).is_some_and(|s|s.eq_ignore_ascii_case("json")){d.error=Some("Save a RAW preset with a .json extension. No file was written.".into());}
                            else {d.request=Some(if matches!(kind,Dialog::Export){Request::Export(path)}else{Request::SavePreset(path)});d.committing=true;}
                        },Ok(None)=>{},Err(e)=>d.error=Some(format!("The RAW save dialog failed: {e}. Your adjustments are still open."))
                    }
                }
                cx.invalidate();
            }),
            Err(e)=>if let Some(d)=&mut self.develop {d.error=Some(format!("Could not open the RAW save dialog: {e}"));},
        }
    }
}
