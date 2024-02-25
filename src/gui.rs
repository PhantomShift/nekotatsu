use iced::{advanced::Widget, widget::{self, text}, Application, Command};

#[derive(Debug, Clone)]
pub enum Message {
    InputFileUpdated(String),
    OutputFileUpdated(String),
    
    SelectFilePressed,
    SelectOutputPressed,
    ConvertPressed,
    UpdatePressed,

    FileSelected(Option<String>),
    DirectorySelected(Option<String>),

    ViewOutputToggled(bool),
    VerboseOutputToggled(bool),

    BackdropClicked,
}

pub struct Nekotatsu {
    input_file: String,
    output_path: String,
    conversion_completed: bool,
    update_completed: bool,
    conversion_output: Option<String>,
    command_output: String,
    should_view_output: bool,
    verbose_output: bool,
}

impl Default for Nekotatsu {
    fn default() -> Self {
        Nekotatsu {
            input_file: String::new(),
            output_path: String::new(),
            conversion_completed: false,
            update_completed: false,
            conversion_output: None,
            command_output: String::new(),
            should_view_output: true,
            verbose_output: true
        }
    }
}

impl Application for Nekotatsu {
    type Message = Message;
    type Theme = iced::theme::Theme;
    type Executor = iced::executor::Default;
    type Flags = ();

    fn new(flags: Self::Flags) -> (Self, iced::Command<Self::Message>) {
        (Nekotatsu::default(), Command::none())
    }

    fn title(&self) -> String {
        String::from("Nekotatsu")
    }

    fn update(&mut self, message: Self::Message) -> Command<Self::Message> {
        match message {
            Message::InputFileUpdated(path) => {
                self.input_file = path;
                Command::none()
            },
            Message::OutputFileUpdated(path) => {
                self.output_path = path;
                Command::none()
            },
            Message::SelectFilePressed => {
                Command::perform(select_file(), Message::FileSelected)
            }
            Message::SelectOutputPressed => {
                Command::perform(select_output(), Message::DirectorySelected)
            }
            Message::FileSelected(file_path) => {
                if let Some(file_path) = file_path {
                    self.input_file = file_path;
                }
                Command::none()
            }
            Message::DirectorySelected(directory_path) => {
                if let Some(directory_path) = directory_path {
                    self.output_path = directory_path
                }
                Command::none()
            }
            Message::ConvertPressed => {
                if let Ok(command_result) = crate::run_command(crate::Commands::Convert {
                    input: self.input_file.clone(),
                    output: Some(self.output_path.clone()),
                    verbose: self.verbose_output,
                    reverse: false,
                    force: true,
                    print_output: false
                }) {
                    match command_result {
                        crate::CommandResult::Success(path, output) => {
                            self.conversion_output = Some(path);
                            self.command_output = output;
                        }
                        crate::CommandResult::None => self.conversion_output = None,
                    }
                }
                
                self.conversion_completed = true;
                Command::none()
            },
            Message::UpdatePressed => {
                let _ = crate::run_command(crate::Commands::Update {
                    kotatsu_link: String::from("https://github.com/KotatsuApp/kotatsu-parsers/archive/refs/heads/master.zip"),
                    tachi_link: String::from("https://raw.githubusercontent.com/keiyoushi/extensions/repo/index.min.json"),
                    force_download: false
                });

                self.update_completed = true;
                Command::none()
            }
            Message::ViewOutputToggled(toggle) => {
                self.should_view_output = toggle;
                Command::none()
            }
            Message::VerboseOutputToggled(toggle) => {
                self.verbose_output = toggle;
                Command::none()
            }
            Message::BackdropClicked => {
                self.conversion_completed = false;
                self.update_completed = false;
                Command::none()
            }
        }
    }

    fn view(&self) -> iced::Element<'_, Self::Message, Self::Theme, iced::Renderer> {
        let fields_completed = [&self.input_file, &self.output_path].iter().filter(|s| s.is_empty()).count() == 0;
        let is_updated = crate::PROJECT_DIR.data_dir().exists();
        let convert_enabled = fields_completed && is_updated;
        
        iced_aw::Modal::new(
            widget::container(
                widget::column!(
                    widget::button("Update Sources and Parsers")
                        .on_press(Message::UpdatePressed),
                    widget::text_input("Input file", &self.input_file)
                        .on_input(Message::InputFileUpdated),
                    widget::button("Select File")
                        .on_press(Message::SelectFilePressed),
                    widget::text_input("Output Directory", &self.output_path)
                        .on_input(Message::OutputFileUpdated),
                    widget::button("Select Output")
                        .on_press(Message::SelectOutputPressed),
                )
                .push_maybe(if convert_enabled {
                    Some(widget::button("Convert").on_press(Message::ConvertPressed))
                } else {
                    None
                })
                .push_maybe(if !convert_enabled {
                    Some(widget::tooltip(
                    widget::button("Convert"),
                        iced_aw::badge(
                            if fields_completed {
                                widget::text("Must update sources/parsers before converting")
                            } else {
                                widget::text("Enter all fields")
                            },
                        ),
                        iced::widget::tooltip::Position::FollowCursor
                    ))
                } else {
                    None
                })
                .spacing(4)
                .padding(iced::Padding::new(32.0))
                .align_items(iced::alignment::Alignment::Start)
                .push(
                    widget::checkbox("View Command Output", self.should_view_output)
                        .on_toggle(Message::ViewOutputToggled)
                )
                .push(
                    widget::checkbox("Verbose Output", self.verbose_output)
                        .on_toggle_maybe(if self.should_view_output {
                            Some(Message::VerboseOutputToggled)
                        } else {
                            None
                        })
                )
            ).center_x(),
            if self.conversion_completed {
                Some(widget::container(
                    if let Some(path) = &self.conversion_output {
                        iced_aw::card(
                            text("Conversion Completed Successfully"),
                            widget::scrollable(
                                if self.should_view_output {
                                    text(&format!("File Path: {path}\n{}", self.command_output))
                                } else {
                                    text(&format!("File Path: {path}"))
                                }
                            )
                        )
                    } else {
                        iced_aw::card(
                            text("Conversion Failed"),
                            widget::scrollable(
                                text(&self.command_output)
                            )
                        )
                    }
                ))
            } else if self.update_completed {
                Some(widget::container(
                    iced_aw::card(
                        text("Update Completed Successfully"),
                        text("Done downloading and generating files")
                    )
                ))
            } else {
                None
            }
        ).backdrop(Message::BackdropClicked).into()
    }

    fn subscription(&self) -> iced::Subscription<Self::Message> {
        iced::event::listen_with(|event, _status| {
            // This is as of yet not fixed for wayland
            // https://github.com/rust-windowing/winit/issues/1881
            if let iced::Event::Window(_id, iced::window::Event::FileDropped(path)) = event {
                Some(Message::FileSelected(Some(path.display().to_string())))
            } else {
                None
            }
        })
    }
}

async fn select_file() -> Option<String> {
    let file = rfd::AsyncFileDialog::new()
        .add_filter("Tachiyomi Backup", &["tachibk", "proto.gz"])
        .pick_file()
        .await;
    return match file {
        Some(handle) => {
            if let Some(path) = handle.path().to_str() {
                Some(path.to_string())
            } else {
                None
            }
        },
        _ => None 
    }
}

async fn select_output() -> Option<String> {
    let file = rfd::AsyncFileDialog::new()
        .save_file()
        .await;
    return match file {
        Some(handle) => {
            if let Some(path) = handle.path().to_str() {
                Some(path.to_string())
            } else {
                None
            }
        },
        _ => None 
    }
}