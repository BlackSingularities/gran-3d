//! Biblioteka użytkownika: zapisane trasy, ulubione miejsca i zakładki widoku.
//! Plik `%APPDATA%\GRAN\biblioteka.json`, zapisywany po każdej zmianie.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedRoute {
    pub id: u64,
    pub name: String,
    /// Grupa (folder) do porządkowania; pusta – bez grupy.
    pub group: String,
    /// Czas zapisu (sekundy uniksowe).
    pub created: u64,
    /// Punkty planowania (dł., szer., nazwa): po otwarciu trasa wyznacza się od nowa.
    pub waypoints: Vec<(f64, f64, Option<String>)>,
    /// Trasa przez teren zamiast szlakami.
    pub terrain: bool,
    /// Ślad (dł., szer., wys.) dla tras wczytanych z pliku (bez punktów planowania).
    pub track: Vec<(f64, f64, f32)>,
    /// Dystans (m), podejścia (m), czas (h) w chwili zapisu – do listy.
    pub dist: f64,
    pub up: f64,
    pub time: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Place {
    pub id: u64,
    pub name: String,
    pub group: String,
    pub created: u64,
    pub lon: f64,
    pub lat: f64,
    pub e: f32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedView {
    pub id: u64,
    pub name: String,
    pub group: String,
    pub created: u64,
    /// Kamera: dł., szer., wysokość celu, azymut, nachylenie, odległość.
    pub camera: (f64, f64, f64, f64, f64, f64),
    pub style: u32,
    pub exag: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Sort {
    #[default]
    Newest,
    Name,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Library {
    pub routes: Vec<SavedRoute>,
    pub places: Vec<Place>,
    pub views: Vec<SavedView>,
    pub sort: Sort,
    next_id: u64,
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn file() -> PathBuf {
    crate::settings::config_dir().join("biblioteka.json")
}

impl Library {
    pub fn load() -> Self {
        std::fs::read(file())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let f = file();
        if let Some(d) = f.parent() {
            std::fs::create_dir_all(d)?;
        }
        // zapis przez plik tymczasowy: przerwany zapis nie niszczy biblioteki
        let tmp = f.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self).unwrap_or_default())?;
        std::fs::rename(&tmp, &f)
    }

    pub fn next_id(&mut self) -> u64 {
        let top = self
            .routes
            .iter()
            .map(|r| r.id)
            .chain(self.places.iter().map(|p| p.id))
            .chain(self.views.iter().map(|v| v.id))
            .max()
            .unwrap_or(0);
        self.next_id = self.next_id.max(top) + 1;
        self.next_id
    }

    /// Wszystkie nazwy grup (posortowane, bez pustej).
    pub fn groups(&self) -> Vec<String> {
        let mut g: Vec<String> = self
            .routes
            .iter()
            .map(|r| r.group.clone())
            .chain(self.places.iter().map(|p| p.group.clone()))
            .chain(self.views.iter().map(|v| v.group.clone()))
            .filter(|g| !g.trim().is_empty())
            .collect();
        g.sort_by_key(|s| s.to_lowercase());
        g.dedup();
        g
    }

    /// Kolejność wyświetlania: indeksy posortowane według ustawienia.
    pub fn order<T>(&self, items: &[T], key: impl Fn(&T) -> (&str, u64)) -> Vec<usize> {
        let mut idx: Vec<usize> = (0..items.len()).collect();
        match self.sort {
            Sort::Newest => idx.sort_by(|&a, &b| key(&items[b]).1.cmp(&key(&items[a]).1)),
            Sort::Name => idx.sort_by_key(|&i| key(&items[i]).0.to_lowercase()),
        }
        idx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_roundtrip() {
        let mut l = Library::default();
        let a = l.next_id();
        l.places.push(Place { id: a, name: "Rysy".into(), lon: 20.08, lat: 49.18, ..Default::default() });
        let b = l.next_id();
        assert!(b > a);
        let text = serde_json::to_string(&l).unwrap();
        let mut back: Library = serde_json::from_str(&text).unwrap();
        assert_eq!(back.places[0].name, "Rysy");
        assert!(back.next_id() > b);
        // stare pliki bez pól nadal się wczytują
        let old: Library = serde_json::from_str("{\"routes\":[{\"name\":\"x\"}]}").unwrap();
        assert_eq!(old.routes[0].name, "x");
    }

    #[test]
    fn order_by_name() {
        let mut l = Library { sort: Sort::Name, ..Default::default() };
        l.places = vec![
            Place { name: "b".into(), created: 1, ..Default::default() },
            Place { name: "A".into(), created: 2, ..Default::default() },
        ];
        assert_eq!(l.order(&l.places, |p| (p.name.as_str(), p.created)), vec![1, 0]);
        l.sort = Sort::Newest;
        assert_eq!(l.order(&l.places, |p| (p.name.as_str(), p.created)), vec![1, 0]);
    }
}
