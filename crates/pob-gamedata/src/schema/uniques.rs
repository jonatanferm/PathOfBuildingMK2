//! `data.uniques` (raw unique item texts per category, incl. the generated
//! ones) and `data.rares` (Data/Rares.lua templates). The items themselves are
//! parsed by the Item class (IT-A); here they are text.

use crate::value::AStr;
use crate::Name;

/// `data.uniques[category]` = list of raw item texts, categories in dump order.
pub type UniqueTexts = indexmap::IndexMap<Name, Vec<AStr>>;
