//! Narzędzia mapy (eksploracja, trasa, pomiar) za wspólnym interfejsem [`MapTool`] oraz soczewki.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tool {
    Explore,
    Route,
    Measure,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Lens {
    None,
    Band,
    Rel,
    Vis,
}
