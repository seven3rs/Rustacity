//! Views for the Rustacity iced shell.
//!
//! The first vertical slice is deliberately one window: a transport row, a
//! waveform canvas and a status bar. The canvas owns hit-testing and drawing;
//! the application owns the view range and the playback position, so every
//! interaction is a plain message.

use iced::mouse;
use iced::widget::{button, canvas, column, container, row, space, text};
use iced::{Color, Element, Length, Point, Rectangle, Size};
use rustacity_engine::Transport;
use rustacity_io::PeakCache;

/// Every action the shell can send to the application.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Message {
    Open,
    Play,
    Stop,
    Record,
    Seek(f64),
    Zoom { factor: f32, anchor: f64 },
    Pan(f64),
    Fit,
}

/// The slice of the loaded track the waveform needs.
#[derive(Debug, Clone, Copy)]
pub struct WaveformView<'a> {
    pub peaks: &'a PeakCache,
    pub position: f64,
    pub view_start: f64,
    pub view_end: f64,
}

/// Everything the window body renders.
#[derive(Debug, Clone, Copy)]
pub struct ViewState<'a> {
    pub status: &'a str,
    pub transport: Transport,
    pub waveform: Option<WaveformView<'a>>,
    pub position: f64,
    pub duration: f64,
}

/// Render the whole window body.
pub fn view<'a>(state: ViewState<'a>) -> Element<'a, Message> {
    let title = row![text("Rustacity").size(24), text("Rust shell").size(14)]
        .spacing(12)
        .padding(12);

    let transport_label = if state.transport.is_playing() {
        "Pause"
    } else {
        "Play"
    };

    let toolbar = row![
        button("Open…").on_press(Message::Open),
        button(transport_label).on_press(Message::Play),
        button("Stop").on_press(Message::Stop),
        button("Record").on_press(Message::Record),
        button("Fit").on_press(Message::Fit),
        button("−").on_press(Message::Zoom {
            factor: 1.5,
            anchor: state.position,
        }),
        button("+").on_press(Message::Zoom {
            factor: 0.75,
            anchor: state.position,
        }),
        space().width(Length::Fill),
        text(format!(
            "{} / {}",
            format_time(state.position),
            format_time(state.duration)
        )),
    ]
    .spacing(8)
    .padding(8)
    .align_y(iced::alignment::Vertical::Center);

    let waveform: Element<'a, Message> = match state.waveform {
        Some(waveform) => canvas::Canvas::new(Waveform { view: waveform })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        None => container(text("Open a WAV file to see its waveform"))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(container::rounded_box)
            .into(),
    };

    let waveform = container(waveform)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(8);

    let status_bar = container(text(state.status)).padding(8).width(Length::Fill);

    column![title, toolbar, waveform, status_bar].into()
}

struct Waveform<'a> {
    view: WaveformView<'a>,
}

impl Waveform<'_> {
    fn time_at(&self, x: f32, width: f32) -> f64 {
        let span = (self.view.view_end - self.view.view_start).max(f64::EPSILON);
        let ratio = (x / width.max(1.0)).clamp(0.0, 1.0) as f64;

        self.view.view_start + ratio * span
    }
}

impl canvas::Program<Message> for Waveform<'_> {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let canvas::Event::Mouse(event) = event else {
            return None;
        };

        match event {
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                let position = cursor.position_in(bounds)?;
                let time = self.time_at(position.x, bounds.width);

                Some(canvas::Action::publish(Message::Seek(time)).and_capture())
            }
            mouse::Event::WheelScrolled { delta } => {
                let (x, y) = match delta {
                    mouse::ScrollDelta::Lines { x, y } => (*x, *y),
                    mouse::ScrollDelta::Pixels { x, y } => (*x / 50.0, *y / 50.0),
                };

                if x.abs() > y.abs() && x != 0.0 {
                    let seconds_per_pixel = (self.view.view_end - self.view.view_start)
                        / f64::from(bounds.width.max(1.0));
                    let seconds = -f64::from(x) * seconds_per_pixel * 40.0;

                    Some(canvas::Action::publish(Message::Pan(seconds)).and_capture())
                } else if y != 0.0 {
                    let position = cursor.position_in(bounds)?;
                    let anchor = self.time_at(position.x, bounds.width);
                    let factor = if y > 0.0 { 0.8 } else { 1.25 };

                    Some(canvas::Action::publish(Message::Zoom { factor, anchor }).and_capture())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let width = bounds.width.max(1.0);
        let height = bounds.height.max(1.0);
        let ruler_height = 24.0f32;

        frame.fill_rectangle(
            Point::ORIGIN,
            Size::new(width, height),
            Color::from_rgb(0.055, 0.063, 0.078),
        );
        frame.fill_rectangle(
            Point::ORIGIN,
            Size::new(width, ruler_height),
            Color::from_rgb(0.09, 0.10, 0.13),
        );

        let visible = (self.view.view_end - self.view.view_start).max(f64::EPSILON);

        for division in 0..=8 {
            let x = width * division as f32 / 8.0;
            let time = self.view.view_start + visible * f64::from(division) / 8.0;

            frame.stroke(
                &canvas::Path::line(Point::new(x, 18.0), Point::new(x, ruler_height)),
                canvas::Stroke::default()
                    .with_width(1.0)
                    .with_color(Color::from_rgb(0.22, 0.24, 0.29)),
            );
            frame.fill_text(canvas::Text {
                content: format_time(time),
                position: Point::new(x + 3.0, 3.0),
                color: Color::from_rgb(0.62, 0.65, 0.70),
                size: 10.0.into(),
                ..canvas::Text::default()
            });
        }

        let drawable_height = (height - ruler_height).max(1.0);
        let center = ruler_height + drawable_height / 2.0;
        let amplitude = drawable_height * 0.45;

        frame.fill_rectangle(
            Point::new(0.0, center),
            Size::new(width, 1.0),
            Color::from_rgb(0.18, 0.20, 0.24),
        );

        let columns = width.round() as usize;
        let peaks = self
            .view
            .peaks
            .peaks(self.view.view_start, self.view.view_end, columns);

        if !peaks.is_empty() {
            let path = canvas::Path::new(|builder| {
                for (column, peak) in peaks.iter().enumerate() {
                    let x = column as f32 + 0.5;
                    let top = (center - peak.max.clamp(-1.0, 1.0) * amplitude)
                        .clamp(ruler_height, height);
                    let bottom = (center - peak.min.clamp(-1.0, 1.0) * amplitude)
                        .clamp(ruler_height, height);
                    let bottom = bottom.max(top + 0.5).min(height);

                    builder.move_to(Point::new(x, top));
                    builder.line_to(Point::new(x, bottom));
                }
            });

            frame.stroke(
                &path,
                canvas::Stroke::default()
                    .with_width(1.0)
                    .with_color(Color::from_rgb(0.42, 0.72, 0.92)),
            );
        }

        if self.view.position >= self.view.view_start && self.view.position <= self.view.view_end {
            let x = ((self.view.position - self.view.view_start) / visible) as f32 * width;
            let path = canvas::Path::line(Point::new(x, ruler_height), Point::new(x, height));

            frame.stroke(
                &path,
                canvas::Stroke::default()
                    .with_width(2.0)
                    .with_color(Color::from_rgb(1.0, 0.35, 0.35)),
            );
        }

        vec![frame.into_geometry()]
    }
}

/// Formats a position as `m:ss.ss` or `h:mm:ss.ss`.
pub fn format_time(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    let hours = (seconds / 3600.0).floor() as u64;
    let minutes = ((seconds % 3600.0) / 60.0).floor() as u64;
    let seconds = seconds % 60.0;

    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:05.2}")
    } else {
        format!("{minutes}:{seconds:05.2}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_short_and_long_positions() {
        assert_eq!(format_time(0.0), "0:00.00");
        assert_eq!(format_time(65.25), "1:05.25");
        assert_eq!(format_time(3661.5), "1:01:01.50");
    }
}
