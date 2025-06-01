#![windows_subsystem = "windows"]

use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use bitflags::Flags;
use iced::widget::*;
use iced::*;

#[derive(Debug, Clone)]
enum Message {
    None,
    Batched(Vec<Message>),

    PathPickClicked(String, PathPickType),
    PathPickChanged { id: String, new: String },
    PathPickSelected { id: String, new: String },
    PathPickNoneSelected,

    FavoritesUpdated(String),
    SoftMatchUpdated(bool),

    TabSwitched(Tab),

    UpdatePressed(UpdateOption),
    UpdateFinished(String),

    PopupStart(String),
    PopupEnd,

    LogAction(text_editor::Action),
    LogAppend(String),
    LogClear,
    LogSave,
    LogSavePicked(PathBuf),

    ExecutePressed,
}

#[derive(Debug, Clone, Copy)]
enum PathPickType {
    FileSelect,
    FileCreate,
    DirectorySelect,
}

#[derive(Debug, Clone)]
enum UpdateOption {
    Tachiyhomi,
    Kotatsu,
    Script,
}

// State that shouldn't be saved between sessions
#[derive(Debug, Default)]
struct TransientState {
    path_picking: bool,
    downloading: bool,
    showing_popup: bool,
    current_tab: Tab,
    popup_message: String,
    log: text_editor::Content,
}

#[derive(Debug)]
struct Animations {
    popup: Animation<bool>,
}

impl Default for Animations {
    fn default() -> Self {
        Animations {
            popup: Animation::new(false).slow(),
        }
    }
}

#[derive(Debug, Default)]
struct Settings {
    tachi_url: Option<String>,
    kotatsu_url: Option<String>,
    script_url: Option<String>,

    favorites_name: String,
    soft_match: bool,
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialOrd, PartialEq)]
    struct Tab : u8 {
        const Main = 1;
        const Results = 2;
        const About = 3;
    }
}

impl Default for Tab {
    fn default() -> Self {
        Tab::Main
    }
}

#[derive(Debug)]
struct App {
    path_pick_entries: HashMap<String, String>,
    about: Vec<markdown::Item>,
    now: Instant,
    settings: Settings,
    animations: Animations,
    transient: TransientState,
    markdown_viewer: CustomViewer,
}

impl Default for App {
    fn default() -> Self {
        let about_text = format!(
            r#"
# Nekotatsu GUI

![la creatura](file:///assets/logo.png)

GUI front-end for a simple tool that converts Neko backups into Kotatsu backups made with 🧊 [Iced](https://github.com/iced-rs/iced).

Version: {}

Repository: [{}]({})
"#,
            env!("CARGO_PKG_VERSION"),
            env!("CARGO_PKG_REPOSITORY"),
            env!("CARGO_PKG_REPOSITORY"),
        );

        Self {
            path_pick_entries: HashMap::new(),
            about: markdown::parse(&about_text).collect(),
            transient: TransientState::default(),
            now: Instant::now(),
            settings: Settings::default(),
            animations: Animations::default(),
            markdown_viewer: CustomViewer::default(),
        }
    }
}

impl App {
    fn boot() -> Self {
        Self::default()
    }

    fn update(&mut self, message: Message, now: Instant) -> Task<Message> {
        self.now = now;
        self.transient.showing_popup &= self.animations.popup.is_animating(now);

        match message {
            Message::Batched(messages) => {
                let mut tasks = Task::none();
                for message in messages {
                    tasks = tasks.chain(self.update(message, now));
                }
                return tasks;
            }

            Message::PathPickClicked(id, pick_type) => {
                if !self.transient.path_picking {
                    self.transient.path_picking = true;
                    return Task::future((async move || {
                        let dialog = rfd::AsyncFileDialog::new();
                        let result = match pick_type {
                            PathPickType::FileSelect => dialog.pick_file().await,
                            PathPickType::FileCreate => dialog.save_file().await,
                            PathPickType::DirectorySelect => dialog.pick_folder().await,
                        };

                        if let Some(handle) = result {
                            Message::PathPickSelected {
                                id,
                                new: handle.path().display().to_string(),
                            }
                        } else {
                            Message::PathPickNoneSelected
                        }
                    })());
                }
            }

            Message::PathPickSelected { id, new } => {
                self.path_pick_entries.insert(id, new);
                self.transient.path_picking = false;
            }
            Message::PathPickNoneSelected => {
                self.transient.path_picking = false;
            }
            Message::PathPickChanged { id, new } => {
                self.path_pick_entries.insert(id, new);
            }

            Message::FavoritesUpdated(name) => self.settings.favorites_name = name,
            Message::SoftMatchUpdated(toggled) => self.settings.soft_match = toggled,

            Message::TabSwitched(tab) => self.transient.current_tab = tab,

            Message::UpdatePressed(option) if !self.transient.downloading => {
                self.transient.downloading = true;

                match log::setup_log_reader() {
                    Ok((mut reader, subscriber)) => {
                        let command = nekotatsu::command::Commands::Update {
                            kotatsu_link: self.settings.kotatsu_url.clone().unwrap_or_else(|| {
                                nekotatsu::command::DEFAULT_KOTATSU_DOWNLOAD.to_string()
                            }),
                            tachi_link: self.settings.tachi_url.clone().unwrap_or_else(|| {
                                nekotatsu::command::DEFAULT_TACHI_DOWNLOAD.to_string()
                            }),
                            script_link: self.settings.script_url.clone().unwrap_or_else(|| {
                                nekotatsu::command::DEFAULT_SCRIPT_DOWNLOAD.to_string()
                            }),
                            force_download: false,
                            force_kotatsu: matches!(option, UpdateOption::Kotatsu),
                            force_tachi: matches!(option, UpdateOption::Tachiyhomi),
                            force_script: matches!(option, UpdateOption::Script),
                        };

                        return Task::done(Message::LogClear).chain(Task::future(
                            (async move || {
                                let result =
                                    nekotatsu::nekotatsu_core::tracing::subscriber::with_default(
                                        subscriber,
                                        || nekotatsu::command::run_command(command),
                                    );
                                let mut log = String::new();
                                if let Err(e) = reader.read_to_string(&mut log) {
                                    log.push_str(&format!(
                                        "\nbroken pipe when reading log; {e:#?}"
                                    ));
                                }
                                if let Err(e) = result {
                                    log.push_str("Error updating, original error:");
                                    log.push_str(&format!("{e:#?}"));
                                }

                                drop(reader);

                                Message::UpdateFinished(log)
                            })(),
                        ));
                    }
                    Err(e) => {
                        self.transient.downloading = false;
                        let err_str = format!("Error updating: {e:#?}");
                        return Task::done(Message::LogClear)
                            .chain(Task::done(Message::LogAppend(err_str.clone())))
                            .chain(Task::done(Message::PopupStart(err_str)));
                    }
                }
            }
            Message::UpdatePressed(_) => (),
            Message::UpdateFinished(log) => {
                self.transient.downloading = false;
                return Task::done(Message::Batched(vec![
                    Message::LogClear,
                    Message::LogAppend(log),
                    Message::PopupStart("Update finished!".to_string()),
                ]));
            }

            Message::LogAction(action) => {
                self.transient.log.perform(action);
            }
            Message::LogAppend(s) => {
                let log = &mut self.transient.log;
                // Move twice in case selected
                log.perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd));
                log.perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd));
                log.perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                    std::sync::Arc::new(s),
                )));
            }
            Message::LogClear => {
                let log = &mut self.transient.log;
                log.perform(text_editor::Action::SelectAll);
                log.perform(text_editor::Action::Edit(text_editor::Edit::Backspace));
            }
            Message::LogSave => {
                return Task::future((async || match rfd::AsyncFileDialog::new()
                    .add_filter("Text File", &["txt", "log"])
                    .save_file()
                    .await
                {
                    Some(handle) => Message::LogSavePicked(handle.path().to_path_buf()),
                    None => Message::None,
                })());
            }
            Message::LogSavePicked(path) => {
                let contents = self.transient.log.text();
                return Task::future((async move || match std::fs::write(path, contents) {
                    Ok(()) => Message::PopupStart("Saved logs!".to_string()),
                    Err(e) => Message::PopupStart(format!("Error writing logs: {e:#?}")),
                })());
            }

            Message::PopupStart(msg) => {
                self.transient.popup_message = msg;
                self.animations.popup = Animation::new(false)
                    .duration(Duration::from_secs(6))
                    .go(true, self.now);
                self.transient.showing_popup = true;
            }
            Message::PopupEnd => {
                self.transient.showing_popup = false;
            }

            Message::ExecutePressed => {
                // TODO: check if currently converting

                let input = self
                    .path_pick_entries
                    .get("input_path")
                    .map(String::to_owned);
                let output = self
                    .path_pick_entries
                    .get("output_path")
                    .map(String::to_owned);

                match (input, output) {
                    (None, _) => {
                        return Task::done(Message::PopupStart("Missing input path".to_string()));
                    }
                    (_, None) => {
                        return Task::done(Message::PopupStart("Missing output path".to_string()));
                    }
                    (Some(input), Some(output)) => {
                        match log::setup_log_reader() {
                            Ok((mut reader, subscriber)) => {
                                let command = nekotatsu::command::Commands::Convert {
                                    input,
                                    output: Some(output),
                                    // TODO: Add settings
                                    favorites_name: if self.settings.favorites_name.is_empty() {
                                        "Favorites".to_string()
                                    } else {
                                        self.settings.favorites_name.clone()
                                    },
                                    verbose: true,
                                    very_verbose: true,
                                    reverse: false,
                                    soft_match: self.settings.soft_match,
                                    force: true,
                                    config_file: None,
                                    disable_default_subscriber: true,
                                };

                                return Task::future((async move || {
                                    let result = nekotatsu::nekotatsu_core::tracing::subscriber::with_default(
                                        subscriber,
                                        || nekotatsu::command::run_command(command)
                                    );
                                    let mut log = String::new();
                                    if let Err(e) = reader.read_to_string(&mut log) {
                                        log.push_str(&format!(
                                            "\nbroken pipe when reading log; {e:#?}"
                                        ));
                                    }
                                    drop(reader);
                                    match result {
                                        Ok(nekotatsu::command::CommandResult::Success(
                                            output_path,
                                        )) => Message::Batched(vec![
                                            Message::LogClear,
                                            Message::LogAppend(log),
                                            Message::PopupStart(format!(
                                                "Converted backup saved to {output_path}, check Results tab for details."
                                            )),
                                        ]),
                                        Ok(nekotatsu::command::CommandResult::None) => {
                                            Message::PopupStart(
                                                "Something has gone horribly wrong.".to_string(),
                                            )
                                        }
                                        Err(e) => {
                                            log.push_str("Error converting, original error:");
                                            log.push_str(&format!("{e:#?}"));
                                            Message::Batched(vec![
                                                Message::LogClear,
                                                Message::LogAppend(log),
                                                Message::PopupStart(format!(
                                                    "Error running command, check Results tab: {e:#?}"
                                                )),
                                            ])
                                        }
                                    }
                                })());
                            }
                            Err(e) => {
                                self.transient.downloading = false;
                                let err_str = format!("Error updating: {e:#?}");
                                return Task::done(Message::LogClear)
                                    .chain(Task::done(Message::LogAppend(err_str.clone())))
                                    .chain(Task::done(Message::PopupStart(err_str)));
                            }
                        }
                    }
                }
            }

            Message::None => (),
        }

        Task::none()
    }

    fn view(&self) -> Element<Message> {
        let mut root = stack!(crate::column![
            row(Tab::FLAGS.iter().map(|tab| {
                button(tab.name())
                    .on_press(Message::TabSwitched(*tab.value()))
                    .style(|theme: &Theme, _status| {
                        let (text, bg) = if *tab.value() == self.transient.current_tab {
                            (
                                theme.extended_palette().primary.base.text,
                                theme.extended_palette().primary.base.color,
                            )
                        } else {
                            (
                                theme.extended_palette().secondary.base.text,
                                theme.extended_palette().secondary.base.color,
                            )
                        };
                        button::Style {
                            text_color: text,
                            background: Some(Background::Color(bg)),
                            border: Border::default(),
                            shadow: Shadow::default(),
                            snap: false,
                        }
                    })
                    .into()
            })),
            container(
                container(match self.transient.current_tab {
                    Tab::Main => self.view_main(),
                    Tab::Results => self.view_results(),
                    Tab::About => self.view_about(),
                    _ => unreachable!("tab should never be outside of range"),
                })
                .padding(8)
                .style(
                    |theme| container::bordered_box(theme).background(theme.palette().background)
                ),
            )
            .padding(8),
            // vertical_space(),
            row![
                horizontal_space(),
                text(format!("v{}", env!("CARGO_PKG_VERSION")))
            ]
            .padding(8)
        ]);

        if self.transient.showing_popup {
            let transparency = match self.animations.popup.interpolate(0.0, 6.0, self.now) {
                ..0.0 => 0.0,
                val @ 0.0..0.2 => val / 0.2,
                0.2..5.8 => 1.0,
                val @ 5.8..6.0 => (6.0 - val) / 0.2,
                _ => 0.0,
            };

            root = root.push(row![
                horizontal_space(),
                crate::column![opaque(
                    container(crate::column![
                        container(crate::column![
                            text(&self.transient.popup_message),
                            row![
                                horizontal_space(),
                                button("Dismiss").on_press(Message::PopupEnd).style(
                                    move |theme, status| {
                                        let mut secondary = button::secondary(theme, status);
                                        secondary = secondary.with_background(
                                            secondary
                                                .background
                                                .unwrap_or(Background::Color(
                                                    theme.palette().background,
                                                ))
                                                .scale_alpha(transparency),
                                        );
                                        secondary.text_color =
                                            secondary.text_color.scale_alpha(transparency);

                                        secondary
                                    }
                                )
                            ],
                        ])
                        .padding(20),
                        progress_bar(
                            0.0..=1.0,
                            self.animations.popup.interpolate(0.0, 1.0, self.now)
                        )
                        .girth(8)
                        .style(move |theme: &Theme| {
                            let bg = theme.extended_palette().background;
                            progress_bar::Style {
                                background: Background::Color(
                                    bg.weakest.color.scale_alpha(transparency),
                                ),
                                bar: Background::Color(bg.strong.color.scale_alpha(transparency)),
                                border: Border::default(),
                            }
                        }),
                    ])
                    .style(move |theme| {
                        let mut style = container::rounded_box(theme);
                        style.text_color = Some(theme.palette().text.scale_alpha(transparency));
                        style = style.background(
                            style
                                .background
                                .unwrap_or(Background::Color(theme.palette().background))
                                .scale_alpha(transparency),
                        );
                        style
                    }),
                ),]
                .padding(16),
                horizontal_space()
            ]);
        }

        root.into()
    }

    fn view_main(&self) -> Element<Message> {
        #[cfg(target_os = "windows")]
        let input_placeholder = "C:\\Path\\to\\backup.proto.gz";
        #[cfg(target_os = "windows")]
        let output_placeholder = "C:\\Path\\to\\output.zip";

        #[cfg(not(target_os = "windows"))]
        let input_placeholder = "/path/to/backup.proto.gz";
        #[cfg(not(target_os = "windows"))]
        let output_placeholder = "/path/to/output.zip";

        crate::column![
            self.path_picker(
                "input_path",
                PathPickType::FileSelect,
                "Input Path",
                input_placeholder
            ),
            self.path_picker(
                "output_path",
                PathPickType::FileCreate,
                "Output Path",
                output_placeholder
            ),
            button("Convert").on_press_maybe({
                if self.path_pick_entries.get("input_path").is_some()
                    && self.path_pick_entries.get("output_path").is_some()
                {
                    Some(Message::ExecutePressed)
                } else {
                    None
                }
            }),
            text("Update"),
            crate::column![
                button("Update Tachiyomi Extensions List")
                    .on_press(Message::UpdatePressed(UpdateOption::Tachiyhomi)),
                button("Update Kotatsu Parsers List")
                    .on_press(Message::UpdatePressed(UpdateOption::Kotatsu)),
                button("Update Resolver Script")
                    .on_press(Message::UpdatePressed(UpdateOption::Script)),
            ]
            .spacing(4),
            text("Settings"),
            crate::column![
                text("Favorite Category Name"),
                text_input("Library", &self.settings.favorites_name)
                    .on_input(Message::FavoritesUpdated),
            ],
            tooltip(
                checkbox("Soft Match", self.settings.soft_match)
                    .on_toggle(Message::SoftMatchUpdated),
                container("Allows comparing links with top-level domains removed (i.e. example.org == example.com)")
                    .padding(4)
                    .style(container::bordered_box),
                tooltip::Position::Top
            ),
        ]
        .height(Fill)
        .spacing(8)
        .into()
    }

    fn view_results(&self) -> Element<Message> {
        crate::column![
            text("Log"),
            scrollable(
                text_editor(&self.transient.log)
                    .placeholder("Currently empty...")
                    .on_action(|action| {
                        match action {
                            text_editor::Action::Edit(_) => Message::None,
                            action => Message::LogAction(action),
                        }
                    })
                    .wrapping(text::Wrapping::Glyph)
                    .height(Length::Shrink),
            )
            .height(Length::Fill)
            .spacing(4)
            .anchor_bottom(),
            row![
                button("Clear").on_press(Message::LogClear),
                button("Save").on_press(Message::LogSave)
            ],
        ]
        .into()
    }

    fn view_about(&self) -> Element<Message> {
        container(markdown::view_with(
            &self.about,
            markdown::Settings::with_style(markdown::Style::from_palette(self.theme().palette())),
            &self.markdown_viewer,
        ))
        .height(Fill)
        .width(Fill)
        .into()
    }

    fn theme(&self) -> Theme {
        Theme::Ferra
    }

    fn subscription(&self) -> Subscription<Message> {
        if self.transient.showing_popup {
            window::frames().map(|_instant| Message::None)
        } else {
            Subscription::none()
        }
    }

    fn path_picker<'a>(
        &self,
        id: &'a str,
        pick_type: PathPickType,
        title: &'a str,
        placeholder: &str,
    ) -> Element<'a, Message> {
        let current = self
            .path_pick_entries
            .get(id)
            .map(String::as_str)
            .unwrap_or_default();

        crate::column![
            text(title),
            row![
                button("Select").on_press(Message::PathPickClicked(id.to_string(), pick_type)),
                text_input(placeholder, current).on_input(|new| Message::PathPickChanged {
                    id: id.to_string(),
                    new
                })
            ]
        ]
        .into()
    }
}

#[derive(Debug)]
struct CustomViewer {
    images: HashMap<&'static str, image::Handle>,
}

impl Default for CustomViewer {
    fn default() -> Self {
        let bytes = include_bytes!("../../assets/logo.png");
        let handle = image::Handle::from_bytes(bytes.as_ref());

        let images = HashMap::from_iter([("/assets/logo.png", handle)]);

        Self { images }
    }
}

impl<'a> markdown::Viewer<'a, Message> for CustomViewer {
    fn on_link_click(url: markdown::Url) -> Message {
        let _ = open::that_detached(url.as_str());

        Message::None
    }

    fn image(
        &self,
        settings: markdown::Settings,
        url: &'a markdown::Url,
        title: &'a str,
        alt: &markdown::Text,
    ) -> Element<'a, Message, Theme, Renderer> {
        // TODO: Just include the image directly instead of messing around with the markdown reader
        if let Some(img) = self.images.get(url.path()) {
            image(img).expand(false).width(400).height(400).into()
        } else {
            markdown::Viewer::image(self, settings, url, title, alt)
        }
    }
}

mod log {
    use std::{io::PipeWriter, sync::Arc};

    use tracing_subscriber::{
        FmtSubscriber,
        filter::LevelFilter,
        fmt::format::{Compact, DefaultFields, Format},
    };

    pub fn setup_log_reader() -> std::io::Result<(
        std::io::PipeReader,
        FmtSubscriber<DefaultFields, Format<Compact, ()>, LevelFilter, Arc<PipeWriter>>,
    )> {
        let (reader, writer) = std::io::pipe()?;
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_file(false)
            .without_time()
            .compact()
            .with_writer(std::sync::Arc::new(writer))
            .finish();
        Ok((reader, subscriber))
    }
}

fn main() -> iced::Result {
    tracing_subscriber::fmt().compact().init();
    iced::application::timed(App::boot, App::update, App::subscription, App::view)
        .theme(App::theme)
        .title("nekotatsu")
        .transparent(true)
        .run()
}
