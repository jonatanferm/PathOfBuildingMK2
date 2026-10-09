//! State export for the oracle (calc doc §9.2): `ctx_struct!`, `ProbeState`,
//! and (feature `probe`) `probe!(ctx, "<id>")`.
//!
//! Owner: ORC-X. Stub created by U0: `ctx_struct!` already generates the ctx
//! structs and their `ProbeState::locals()` (the Lua local names, in order), so
//! U0's orchestrators compile; ORC-X adds the export/injection of every field
//! by name (pob-state/1, `crates/pob-oracle/README.md`).
#![allow(non_snake_case)]

/// A segment context whose fields are Lua locals, by name.
pub trait ProbeState {
    /// The field names as written (`r#override` for `override`); see [`ProbeState::locals`].
    const FIELDS: &'static [&'static str];

    /// The verbatim Lua local names, in declaration order.
    fn locals() -> Vec<&'static str> {
        Self::FIELDS
            .iter()
            .map(|f| f.strip_prefix("r#").unwrap_or(f))
            .collect()
    }
}

/// `ctx_struct! { pub struct X { a: A, b: B } }`: the struct with public
/// fields plus its [`ProbeState`] impl.
#[macro_export]
macro_rules! ctx_struct {
    ($(#[$m:meta])* pub struct $name:ident { $($f:ident : $t:ty),* $(,)? }) => {
        $(#[$m])*
        pub struct $name {
            $(pub $f: $t),*
        }
        impl $crate::probe::ProbeState for $name {
            const FIELDS: &'static [&'static str] = &[$(stringify!($f)),*];
        }
    };
}

/// `probe!(ctx, "<id>")`: export the ctx at a stage boundary (feature `probe`;
/// a no-op otherwise). ORC-X implements the export.
#[macro_export]
macro_rules! probe {
    ($ctx:expr, $id:literal) => {{
        #[cfg(feature = "probe")]
        {
            let _ = (&$ctx, $id);
            $crate::unported!("probe export (ORC-X)")
        }
    }};
}
