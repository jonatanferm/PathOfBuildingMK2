//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Modules/Main.lua 1-1822 (the `main` object; its
//! generic-control part is pob_ui_core::main_core::MainCore). Owner: U16 (popups: U17). Module
//! declarations: UI-U0.

pub mod about_popup;
pub mod background;
pub mod folders;
pub mod init;
pub mod on_frame;
pub mod options_popup;
pub mod path_popup;
pub mod settings;
pub mod toast;
pub mod update_popup;

use pob_ui_core::input::InputEvents;

/// Main.lua fields not in MainCore: mode/newMode/modes, settings, paths, sharedItems, option
/// values, inputEvents, toasts... UI-U0: `input_events` (ui doc §6.11: main.inputEvents lives here);
/// U16 defines the rest.
#[derive(Default)]
pub struct MainState {
    /// main.inputEvents
    pub input_events: InputEvents,
}
