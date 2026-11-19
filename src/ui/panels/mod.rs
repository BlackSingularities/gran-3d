//! Interfejs nad mapą: górny pasek (wyszukiwarka, narzędzia, widok, zegar), szuflada (szczegóły, warstwy, światło), sterowanie kamerą, profil i pomoc.

use eframe::egui::{self, Color32, RichText};

use crate::fmt;
use crate::graph::Route;
use crate::geo::{bearing, haversine};
use crate::graph::RouteKind;
use crate::metrics::{self, TimeModel, DIFFICULTY, SAC_LABEL};
use crate::osm::PoiKind;
use crate::sun::{self, Day};
use crate::ui::atlas::{Action, Atlas, Lens, Quality, RouteMode, Selection, Style, Tool};
use crate::ui::profile;
use crate::ui::theme;

mod bar;
mod community;
mod legend;
mod map;
mod mobile;
mod place;
mod record;
mod route;
mod sidebar;
mod widgets;
mod windows;

pub use bar::*;
pub use community::*;
pub use legend::*;
pub use map::*;
pub use mobile::{close_more, map_buttons, round_icon, MAP_BUTTONS_H};
pub use place::*;
pub use record::*;
pub use route::*;
pub use sidebar::*;
pub use widgets::*;
pub use windows::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Info,
    Library,
    Conditions,
    /// Położenie GPS i nagrywanie przejścia (telefon).
    Record,
}
