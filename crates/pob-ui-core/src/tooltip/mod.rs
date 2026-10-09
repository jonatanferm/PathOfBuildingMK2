//! TooltipHost and Tooltip (ui doc §6.9). Owner: U10. Module declarations are UI-U0's.

pub mod host;
#[allow(clippy::module_inception)]
pub mod tooltip;

pub use host::{TooltipFn, TooltipHost};
pub use tooltip::{
    ModLineRef, Tooltip, TooltipBlock, TooltipDrawFn, TooltipLine, TtBackground, TtColor, TtKey,
    TtObj,
};
