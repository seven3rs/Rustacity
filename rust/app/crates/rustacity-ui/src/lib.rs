//! Views for the Rustacity iced shell.
//!
//! The shell starts as one window with a title bar, a transport row and a
//! placeholder for the waveform. The pane tree, menus and dialogs arrive in
//! M2; keeping the first view small is what makes the iced decision testable
//! before the engine exists.

use iced::widget::{button, column, container, row, text};
use iced::{Element, Length};
use rustacity_engine::Transport;

/// Every action the shell can send to the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Message {
    Open,
    Play,
    Stop,
    Record,
}

/// Render the whole window body.
pub fn view<'a>(status: &'a str, transport: Transport) -> Element<'a, Message> {
    let title = row![text("Rustacity").size(24), text("Rust shell").size(14)]
        .spacing(12)
        .padding(12);

    let transport_label = if transport == Transport::Playing {
        "Pause"
    } else {
        "Play"
    };

    let toolbar = row![
        button("Open…").on_press(Message::Open),
        button(transport_label).on_press(Message::Play),
        button("Stop").on_press(Message::Stop),
        button("Record").on_press(Message::Record),
    ]
    .spacing(8)
    .padding(8);

    let waveform = container(text("Waveform canvas lands in M1"))
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(24)
        .style(container::rounded_box);

    let status_bar = container(text(status)).padding(8).width(Length::Fill);

    column![title, toolbar, waveform, status_bar].into()
}
