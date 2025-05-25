#![windows_subsystem = "windows"]

use rfd;
use slint::{self, ComponentHandle};
use std::io::Read;
use tokio;

use nekotatsu::command::{self, CommandResult, Commands};

mod application {
    slint::include_modules!();
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = tokio::runtime::Runtime::new()?;

    runtime.block_on(async { runtime.spawn_blocking(run_app_inner).await })??;

    Ok(())
}

fn run_app_inner() -> Result<(), slint::PlatformError> {
    let app = application::Application::new()?;

    let cc_handle = app.as_weak();
    app.on_convert_clicked(move || {
        let app = cc_handle.unwrap();
        let input = app.get_in_path().to_string();
        let output = Some(app.get_out_path().to_string());
        let favorites_name = app.get_library_name().to_string();
        let verbose = app.get_verbose_output();
        let view_output = app.get_view_output();
        let cc_handle = app.as_weak();
        app.set_processing(true);
        tokio::spawn(async move {
            let (mut output_buffer, trace_guard, reader) = if view_output {
                let output_buffer = Vec::<u8>::new();
                let (reader, writer) = std::io::pipe().unwrap();
                let subscriber = nekotatsu::tracing_subscriber::fmt()
                    .with_ansi(false)
                    .with_file(false)
                    .with_level(true)
                    .with_target(false)
                    .without_time()
                    .compact()
                    .with_writer(std::sync::Arc::new(writer))
                    .finish();

                let trace_guard =
                    nekotatsu::nekotatsu_core::tracing::subscriber::set_default(subscriber);
                (Some(output_buffer), Some(trace_guard), Some(reader))
            } else {
                (None, None, None)
            };

            let result = command::run_command(Commands::Convert {
                input,
                output,
                favorites_name,
                verbose,
                very_verbose: false,
                reverse: false,
                soft_match: false,
                force: true,
                config_file: None,
            });

            drop(trace_guard);
            if let (Some(mut reader), Some(mut output_buffer)) = (reader, output_buffer.as_mut()) {
                reader
                    .read_to_end(&mut output_buffer)
                    .expect("output be readable");
                drop(reader);
            };

            cc_handle
                .upgrade_in_event_loop(move |app| {
                    app.set_processing(false);
                    match application::ChildWindow::new() {
                        Ok(child) => {
                            match result {
                                Ok(result) => {
                                    if let crate::CommandResult::Success(path) = result {
                                        child.set_description(format!("Saved to '{path}'").into());
                                        if let Some(output_buffer) = output_buffer {
                                            let output =
                                                String::from_utf8_lossy(&output_buffer).to_string();
                                            child.set_lines(output.lines().count() as i32);
                                            child.set_child_text(output.into());
                                            child
                                                .set_init_height(app.window().size().height as i32);
                                        };
                                    }
                                }
                                Err(e) => {
                                    child.set_description(
                                        format!("Stopped with error '{}'", e.to_string()).into(),
                                    );
                                }
                            };
                            let cc_handle = child.as_weak();
                            child.on_close_clicked(move || {
                                let child = cc_handle.unwrap();
                                child.hide().unwrap();
                            });
                            child.window().set_position(app.window().position());
                            child.show().unwrap();
                        }
                        Err(e) => {
                            println!("Error: {e}");
                        }
                    }
                })
                .unwrap();
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
                ic_handle
                    .upgrade_in_event_loop(move |app| {
                        app.set_in_path(path.into());
                    })
                    .unwrap();
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
                oc_handle
                    .upgrade_in_event_loop(move |app| {
                        app.set_out_path(path.into());
                    })
                    .unwrap();
            }
        });
    });

    let uc_handle = app.as_weak();
    app.on_update_clicked(move || {
        let app = uc_handle.unwrap();
        let uc_handle = app.as_weak();
        app.set_processing(true);
        tokio::task::spawn_blocking(move || {
            let _ = command::run_command(Commands::Update {
                kotatsu_link: String::from(
                    "https://github.com/KotatsuApp/kotatsu-parsers/archive/refs/heads/master.zip",
                ),
                tachi_link: String::from(
                    "https://raw.githubusercontent.com/keiyoushi/extensions/repo/index.min.json",
                ),
                script_link: String::from(
                    "https://raw.githubusercontent.com/phantomshift/nekotatsu/nekotatsu-core/src/correction.luau"
                ),
                force_download: false,
                force_kotatsu: false,
                force_tachi: false,
                force_script: false
            });
            uc_handle
                .upgrade_in_event_loop(|app| app.set_processing(false))
                .unwrap();
        });
    });

    app.run()?;
    Ok(())
}
