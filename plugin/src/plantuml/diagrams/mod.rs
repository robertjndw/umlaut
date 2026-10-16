//! One parser and model per diagram type. A parser gets the lines left after
//! `meta::extract` and the shared `Ctx`, registers every text it will draw in
//! `ctx.reg` (always in source order, so the order is a pure function of the source)
//! and returns a model for `layout::<type>` to draw.

pub mod activity;
pub mod class;
pub mod description;
pub mod gcommon;
pub mod sequence;
pub mod state;
