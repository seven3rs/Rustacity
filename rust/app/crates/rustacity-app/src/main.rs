mod app;

use app::App;

fn main() -> iced::Result {
    iced::application(App::new, App::update, App::view)
        .title(App::title)
        .theme(App::theme)
        .window(iced::window::Settings {
            size: iced::Size::new(1200.0, 800.0),
            ..Default::default()
        })
        .run()
}
