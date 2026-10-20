//! The application state and update loop.
//!
//! The app owns the loaded audio, the playback stream and the waveform's view
//! range. `rustacity-ui` only turns that state into widgets and messages, so
//! the same views can be rendered from tests without opening a window.

use std::path::{Path, PathBuf};
use std::time::Duration;

use iced::{Element, Subscription, Task};
use rustacity_engine::{Project, Track, Transport};
use rustacity_io::{AudioFile, AudioPlayer, PeakCache};
use rustacity_ui as ui;

const MIN_VIEW_SPAN: f64 = 0.005;

#[derive(Debug)]
pub struct App {
    project: Project,
    status: String,
    loaded: Option<LoadedTrack>,
    player: Option<AudioPlayer>,
    view_start: f64,
    view_end: f64,
    position: f64,
}

#[derive(Debug)]
pub(crate) struct LoadedTrack {
    name: String,
    file: AudioFile,
    peaks: PeakCache,
}

/// Messages owned by the app: UI actions plus the results of background work.
#[derive(Debug)]
pub enum Message {
    Ui(ui::Message),
    Opened(Option<Result<LoadedTrack, String>>),
    Tick,
}

impl App {
    pub fn new() -> Self {
        Self {
            project: Project::new(),
            status: String::from("Open a WAV file to begin"),
            loaded: None,
            player: None,
            view_start: 0.0,
            view_end: 0.0,
            position: 0.0,
        }
    }

    /// Start the app, opening a WAV passed on the command line if there is one.
    pub fn boot() -> (Self, Task<Message>) {
        let mut app = Self::new();

        let Some(path) = std::env::args_os().nth(1).map(PathBuf::from) else {
            return (app, Task::none());
        };

        app.status = format!("Opening {}…", path.display());

        let task = Task::perform(async move { load_path(path) }, Message::Opened);

        (app, task)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Ui(message) => self.update_ui(message),
            Message::Opened(result) => {
                self.load(result);
                Task::none()
            }
            Message::Tick => {
                self.tick();
                Task::none()
            }
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        if self.player.as_ref().is_some_and(AudioPlayer::is_playing) {
            iced::time::every(Duration::from_millis(33)).map(|_| Message::Tick)
        } else {
            Subscription::none()
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let waveform = self.loaded.as_ref().map(|loaded| ui::WaveformView {
            peaks: &loaded.peaks,
            position: self.position,
            view_start: self.view_start,
            view_end: self.view_end,
        });

        ui::view(ui::ViewState {
            status: &self.status,
            transport: self.project.transport,
            waveform,
            position: self.position,
            duration: self.duration(),
        })
        .map(Message::Ui)
    }

    pub fn title(&self) -> String {
        self.loaded.as_ref().map_or_else(
            || String::from("Rustacity"),
            |loaded| format!("Rustacity — {}", loaded.name),
        )
    }

    pub fn theme(&self) -> iced::Theme {
        iced::Theme::Dark
    }

    fn update_ui(&mut self, message: ui::Message) -> Task<Message> {
        match message {
            ui::Message::Open => {
                self.status = String::from("Choose a WAV file…");
                Task::perform(open_wav(), Message::Opened)
            }
            ui::Message::Play => {
                self.toggle_play();
                Task::none()
            }
            ui::Message::Stop => {
                self.stop();
                Task::none()
            }
            ui::Message::Record => {
                self.status = String::from("Recording is not wired yet");
                Task::none()
            }
            ui::Message::Seek(seconds) => {
                self.seek(seconds);
                Task::none()
            }
            ui::Message::Zoom { factor, anchor } => {
                self.zoom(factor, anchor);
                Task::none()
            }
            ui::Message::Pan(seconds) => {
                self.pan(seconds);
                Task::none()
            }
            ui::Message::Fit => {
                self.fit();
                Task::none()
            }
        }
    }

    fn load(&mut self, result: Option<Result<LoadedTrack, String>>) {
        let Some(result) = result else {
            self.status = String::from("Open cancelled");
            return;
        };

        let loaded = match result {
            Ok(loaded) => loaded,
            Err(error) => {
                self.status = error;
                return;
            }
        };

        let duration = loaded.file.duration_seconds();
        let detail = format!(
            "{}: {} · {} Hz · {} ch",
            loaded.name,
            ui::format_time(duration),
            loaded.file.sample_rate,
            loaded.file.channels
        );

        self.project = Project::new();
        self.project.add_track(Track::new(
            loaded.name.clone(),
            loaded.file.channels,
            loaded.file.sample_rate,
        ));
        self.position = 0.0;
        self.view_start = 0.0;
        self.view_end = duration;

        self.player = match AudioPlayer::new(&loaded.file) {
            Ok(player) => {
                self.status = format!("{detail} — click the waveform to seek");
                Some(player)
            }
            Err(error) => {
                self.status = format!("{detail} — playback unavailable: {error}");
                None
            }
        };

        self.loaded = Some(loaded);
    }

    fn toggle_play(&mut self) {
        let Some(player) = &self.player else {
            self.status = String::from("Open a WAV file first");
            return;
        };

        if player.is_playing() {
            player.pause();
            self.project.stop();
            self.position = player.position_seconds();
            self.status = format!("Paused at {}", ui::format_time(self.position));
        } else {
            if self.position >= self.duration() - 1e-9 {
                player.seek_seconds(0.0);
                self.position = 0.0;
            }

            player.play();
            self.project.play();
            self.status = String::from("Playing");
        }
    }

    fn stop(&mut self) {
        if let Some(player) = &self.player {
            player.stop();
        }

        self.project.stop();
        self.position = 0.0;
        self.status = String::from("Stopped");
    }

    fn seek(&mut self, seconds: f64) {
        let Some(player) = &self.player else {
            return;
        };

        player.seek_seconds(seconds);
        self.position = player.position_seconds();
    }

    fn tick(&mut self) {
        let Some(player) = &self.player else {
            return;
        };

        self.position = player.position_seconds();

        if self.project.transport == Transport::Playing && !player.is_playing() {
            self.project.stop();

            if self.position >= self.duration() - 1e-6 {
                self.status = String::from("Reached the end");
            }
        }
    }

    fn zoom(&mut self, factor: f32, anchor: f64) {
        let duration = self.duration();

        if duration <= 0.0 {
            return;
        }

        let min_span = duration.min(MIN_VIEW_SPAN);
        let current_span = (self.view_end - self.view_start).clamp(min_span, duration);
        let span = (current_span * f64::from(factor)).clamp(min_span, duration);
        let anchor = anchor.clamp(self.view_start, self.view_end);
        let ratio = if current_span > 0.0 {
            (anchor - self.view_start) / current_span
        } else {
            0.5
        };

        let start = if span >= duration {
            0.0
        } else {
            (anchor - ratio * span).clamp(0.0, duration - span)
        };

        self.view_start = start;
        self.view_end = (start + span).min(duration);
    }

    fn pan(&mut self, seconds: f64) {
        let duration = self.duration();

        if duration <= 0.0 {
            return;
        }

        let span = (self.view_end - self.view_start).clamp(0.0, duration);
        let start = (self.view_start + seconds).clamp(0.0, (duration - span).max(0.0));

        self.view_start = start;
        self.view_end = (start + span).min(duration);
    }

    fn fit(&mut self) {
        self.view_start = 0.0;
        self.view_end = self.duration();
    }

    fn duration(&self) -> f64 {
        self.loaded
            .as_ref()
            .map_or(0.0, |loaded| loaded.file.duration_seconds())
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

async fn open_wav() -> Option<Result<LoadedTrack, String>> {
    let handle = rfd::AsyncFileDialog::new()
        .add_filter("WAV audio", &["wav", "WAV"])
        .set_title("Open WAV")
        .pick_file()
        .await?;

    load_path(handle.path().to_path_buf())
}

fn load_path(path: PathBuf) -> Option<Result<LoadedTrack, String>> {
    match rustacity_io::decode_wav(&path) {
        Ok(file) => {
            let peaks = PeakCache::new(&file);

            Some(Ok(LoadedTrack {
                name: file_name(&path),
                file,
                peaks,
            }))
        }
        Err(error) => Some(Err(format!("Could not open {}: {error}", path.display()))),
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Untitled")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_with_duration(duration: f64) -> App {
        let file = AudioFile {
            samples: vec![0.0; 1_000].into(),
            mono: vec![0.0; 1_000].into(),
            channels: 1,
            sample_rate: 1_000,
        };
        let peaks = PeakCache::new(&file);
        let mut app = App::new();
        app.loaded = Some(LoadedTrack {
            name: String::from("test.wav"),
            file,
            peaks,
        });
        app.view_start = 0.0;
        app.view_end = duration;

        app
    }

    #[test]
    fn zoom_keeps_the_anchor_under_the_cursor() {
        let mut app = app_with_duration(1.0);
        app.zoom(0.5, 0.75);

        assert!((app.view_start - 0.375).abs() < 1e-9);
        assert!((app.view_end - 0.875).abs() < 1e-9);
        assert!(((0.75 - app.view_start) / (app.view_end - app.view_start) - 0.75).abs() < 1e-9);
    }

    #[test]
    fn zoom_clamps_to_the_track() {
        let mut app = app_with_duration(1.0);
        app.zoom(0.5, 0.0);
        app.zoom(0.5, 0.0);

        assert_eq!(app.view_start, 0.0);
        assert!(app.view_end <= 1.0);
        assert!(app.view_end > 0.0);
    }

    #[test]
    fn pan_clamps_to_the_track() {
        let mut app = app_with_duration(1.0);
        app.pan(10.0);

        assert_eq!(app.view_start, 0.0);
        assert_eq!(app.view_end, 1.0);

        app.zoom(0.5, 1.0);
        app.pan(-10.0);

        assert_eq!(app.view_start, 0.0);
        assert!((app.view_end - 0.5).abs() < 1e-9);
    }
}
