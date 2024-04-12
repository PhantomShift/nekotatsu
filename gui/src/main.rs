use slint;
use tokio;
use rfd;

use nekotatsu::{Commands, CommandResult};

pub mod child_window;

slint::slint!{
import { VerticalBox, Button, LineEdit, CheckBox, TextEdit } from "std-widgets.slint";

component FileButton inherits Button {
    text: "🗂️";
    max-height: 30px;
    max-width: 30px;
}

export component Application inherits Window {
    title: "nekotatsu";

    callback update-clicked();
    callback convert-clicked();
    callback input-clicked();
    callback output-clicked();

    in-out property <string> popup-text;
    in-out property <string> in-path;
    in-out property <string> out-path;

    out property <bool> view-output: true;
    out property <bool> verbose-output: false;

    VerticalLayout {
        Button {
            text: "Update Sources and Parsers";
            clicked => { update-clicked() }
        }
        HorizontalLayout {
            FileButton {
                clicked => { input-clicked() }
            }
            input-path := LineEdit {
                placeholder-text: "/path/to/input";
                text: in-path;
                edited => { in-path = self.text }
            }
        }
        HorizontalLayout {
            FileButton {
                clicked => { output-clicked() }
            }
            output-path := LineEdit {
                placeholder-text: "/path/to/output";
                text: out-path;
                edited => { out-path = self.text }
            }
        }
        Button {
            text: "Convert";
            clicked => {
                convert-clicked()
            }
        }
        CheckBox {
            checked: view-output;
            toggled => { view-output = self.checked }
            text: "View Output";
        }
        CheckBox {
            enabled: view-output;
            checked: verbose-output;
            toggled => { verbose-output = self.checked }
            text: "Verbose Output";
        }
    }
}
}

fn main() -> Result<(), Box::<dyn std::error::Error>> {
    let runtime = tokio::runtime::Runtime::new()?;

    runtime.block_on(async {
        runtime.spawn_blocking(run_app_inner).await
    })??;

    Ok(())
}

fn run_app_inner() -> Result<(), slint::PlatformError> {
    let app = Application::new()?;
    
    let cc_handle = app.as_weak();
    app.on_convert_clicked(move || {
        let app = cc_handle.unwrap();
        let input = app.get_in_path().to_string();
        let output = Some(app.get_out_path().to_string());
        let verbose = app.get_verbose_output();
        let print_output = !app.get_view_output();
        let cc_handle = app.as_weak();
        tokio::spawn(async move {
            let result = nekotatsu::run_command(Commands::Convert {
                input,
                output,
                favorites_name: String::from("Library"),
                verbose,
                reverse: false,
                soft_match: false,
                force: true,
                print_output
            });

            slint::invoke_from_event_loop(move|| {
                let app = cc_handle.unwrap();
                match child_window::ChildWindow::new() {
                    Ok(child) => {
                        match result {
                            Ok(result) => {
                                if let crate::CommandResult::Success(path, output) = result {
                                    child.set_description(format!("Saved to '{path}'").into());
                                    if !print_output {
                                        child.set_child_text(output.into());
                                        child.set_init_height(app.window().size().height as i32);
                                    }
                                }
                            },
                            Err(e) => {
                                child.set_description(format!("Stopped with error '{}'", e.to_string()).into());
                            }
                        };
                        let cc_handle = child.as_weak();
                        child.on_close_clicked(move || {
                            let child = cc_handle.unwrap();
                            child.hide().unwrap();
                        });
                        child.show().unwrap();
                    },
                    Err(e) => {
                        println!("Error: {e}");
                    }
                }
            }).unwrap();
        });
    });

    let ic_handle = app.as_weak();
    app.on_input_clicked(move || {
        let app = ic_handle.unwrap();
        let ic_handle = app.as_weak();
        tokio::spawn(async move {
            let file_handle = rfd::AsyncFileDialog::new()
                .add_filter("Tachiyomi Backup", &["tachibk", "proto.gz"])
                .pick_file()
                .await;
            if let Some(file_handle) = file_handle {
                let path = file_handle.path().display().to_string();
                ic_handle.upgrade_in_event_loop(move |app| {
                    app.set_in_path(path.into());
                }).unwrap();
            }
        });
    });
    let oc_handle = app.as_weak();
    app.on_output_clicked(move || {
        let app = oc_handle.unwrap();
        let oc_handle = app.as_weak();
        tokio::spawn(async move {
            let file_handle = rfd::AsyncFileDialog::new()
                .add_filter("Kotatsu Backup", &["zip"])
                .save_file()
                .await;
            if let Some(file_handle) = file_handle {
                let path = file_handle.path().display().to_string();
                oc_handle.upgrade_in_event_loop(move |app| {
                    app.set_out_path(path.into());
                }).unwrap();
            }
        });
    });
    app.on_update_clicked(|| {
        let _ = nekotatsu::run_command(Commands::Update {
            kotatsu_link: String::from("https://github.com/KotatsuApp/kotatsu-parsers/archive/refs/heads/master.zip"),
            tachi_link: String::from("https://raw.githubusercontent.com/keiyoushi/extensions/repo/index.min.json"),
            force_download: false
        });
    });
    
    app.run()?;
    Ok(())
}