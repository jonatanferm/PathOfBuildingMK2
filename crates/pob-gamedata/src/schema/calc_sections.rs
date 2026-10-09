//! `CalcSections` (Modules/CalcSections.lua): the Calcs tab layout. It is UI
//! data (rows of `{ width, id, group, colour, subsections… }` arrays with
//! format strings and closures) and is kept in its dumped form; the UI's
//! calcs-tab unit types it.

use crate::value::DataValue;

/// The dumped `CalcSections` table.
#[derive(Debug, Clone, Default)]
pub struct CalcSections(pub DataValue);

impl<'de> serde::Deserialize<'de> for CalcSections {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        crate::loader::data_value(d).map(CalcSections)
    }
}
