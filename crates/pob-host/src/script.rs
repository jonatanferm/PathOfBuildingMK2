//! The `.pobscript` scenario DSL shared by `run_headless` and the oracle (ui doc §5.2). Owner: U7.
//!
//! ```text
//! screen 1280 800 dpi 1.0
//! fixture userdir=tests/ui-fixtures/user1        # Settings.xml + Builds/
//! load-build "Builds/witch arc.xml"              # == main:SetMode("BUILD", false, name, xml)
//! frames 2
//! move 650 300
//! click LEFTBUTTON                               # move+frame, down+frame, up+frame
//! key-down CTRL ; key-down 6 ; key-up 6 ; key-up CTRL
//! type "Hello ^1world"                           # per byte: Char (+KeyDown/KeyUp for named keys), as SG delivers
//! press TAB                                      # KeyDown TAB + Char "\t" + frame + KeyUp TAB
//! wheel down 3                                   # 3x (KeyDown+KeyUp WHEELDOWN)
//! paste-clipboard "..."                          # sets the virtual clipboard
//! frames 3
//! snapshot notes_typed                           # dumps build XML from both sides too
//! ```

use pob_sg::Key;

/// One scenario step.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// `screen W H dpi S`
    Screen { w: i32, h: i32, dpi: f32 },
    /// `fixture key=value`
    Fixture { key: String, value: String },
    /// `load-build "path"`
    LoadBuild(String),
    /// `frames N`
    Frames(u32),
    /// `move X Y` (virtual px)
    Move(f64, f64),
    /// `click KEY`
    Click(Key),
    /// `key-down KEY`
    KeyDown(Key),
    /// `key-up KEY`
    KeyUp(Key),
    /// `type "text"`
    Type(String),
    /// `press KEY`
    Press(Key),
    /// `wheel up|down N`
    Wheel { up: bool, count: u32 },
    /// `paste-clipboard "text"`
    PasteClipboard(String),
    /// `snapshot NAME`
    Snapshot(String),
}

/// A parsed `.pobscript`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Script {
    pub steps: Vec<Step>,
}

impl Script {
    /// Parse a `.pobscript` (`#` comments, `;` separates steps on one line).
    pub fn parse(text: &str) -> Result<Script, String> {
        let _ = text;
        todo!("U7: .pobscript parser")
    }

    /// Render back to text (round-trip tests).
    pub fn to_text(&self) -> String {
        todo!("U7: .pobscript printer")
    }
}
