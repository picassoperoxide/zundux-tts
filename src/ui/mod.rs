pub mod input;
pub mod settings;
pub mod soundboard;
pub mod theme;
pub mod titlebar;
pub mod widgets;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Input,
    Soundboard,
    Settings,
}
