//! The M0 shell state.
//!
//! The application owns the state and the update loop; `rustacity-ui` owns
//! the views. Keeping that split means the same views can be rendered from
//! tests without starting a window.

use iced::Element;
use rustacity_engine::{Project, Transport};
use rustacity_ui::Message;

#[derive(Debug)]
pub struct App {
    project: Project,
    status: String,
}

impl App {
    pub fn new() -> Self {
        Self {
            project: Project::new(),
            status: String::from("M0 shell: open a file in M1"),
        }
    }

    pub fn update(&mut self, message: Message) {
        match message {
            Message::Open => {
                self.status = String::from("Open lands in M1");
            }
            Message::Play => {
                if self.project.transport == Transport::Playing {
                    self.project.stop();
                    self.status = String::from("Stopped");
                } else {
                    self.project.play();
                    self.status = String::from("Playing (silent: the audio engine lands in M1)");
                }
            }
            Message::Stop => {
                self.project.stop();
                self.status = String::from("Stopped");
            }
            Message::Record => {
                self.project.record();
                self.status = String::from("Recording is not wired yet");
            }
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        rustacity_ui::view(&self.status, self.project.transport)
    }

    pub fn title(&self) -> String {
        String::from("Rustacity")
    }

    pub fn theme(&self) -> iced::Theme {
        iced::Theme::Dark
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}
