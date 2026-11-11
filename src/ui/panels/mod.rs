//! Interfejs nad mapą: górny pasek (wyszukiwarka, narzędzia, widok, zegar), szuflada (szczegóły, warstwy, światło), sterowanie kamerą, profil i pomoc.

use eframe::egui::{self, Color32, RichText};

use crate::fmt;
use crate::graph::Route;
use crate::geo::{bearing, haversine};
use crate::graph::RouteKind;
use crate::metrics::{self, TimeModel, DIFFICULTY, SAC_LABEL};
use crate::osm::PoiKind;
use crate::sun::{self, Day};
use crate::ui::atlas::{Atlas, Lens, Quality, RouteMode, Selection, Style, Tool};
use crate::ui::profile;
use crate::ui::theme;

mod bar;
mod map;
mod place;
mod route;
mod sidebar;
mod widgets;
mod windows;

pub use bar::*;
pub use map::*;
pub use place::*;
pub use route::*;
pub use sidebar::*;
pub use widgets::*;
pub use windows::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Info,
    Library,
    Conditions,
}
