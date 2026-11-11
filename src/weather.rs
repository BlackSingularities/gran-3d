//! Prognoza pogody dla punktu na zadanej wysokości (Open-Meteo, bez klucza): godzinowa na 7 dni
//! i dobowa. Open-Meteo przelicza temperaturę na wysokość punktu (`elevation`), więc prognoza dla
//! szczytu różni się od prognozy dla doliny. Wyniki trzymane w pamięci przez 30 minut.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;

const API: &str = "https://api.open-meteo.com/v1/forecast";
const TTL: Duration = Duration::from_secs(30 * 60);
const RETRY: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, Default)]
pub struct Hour {
    /// Czas (sekundy uniksowe, UTC).
    pub t: i64,
    pub temp: f32,
    pub feels: f32,
    /// Wiatr i porywy (km/h), kierunek, z którego wieje (°).
    pub wind: f32,
    pub gust: f32,
    pub dir: f32,
    /// Zachmurzenie ogólne i niskie (%).
    pub cloud: f32,
    pub cloud_low: f32,
    /// Opad (mm/h), prawdopodobieństwo opadu (%), śnieg (cm/h).
    pub precip: f32,
    pub prob: f32,
    pub snow: f32,
    /// Wysokość izotermy 0 °C (m n.p.m.).
    pub freezing: f32,
    /// Kod pogody WMO.
    pub code: u8,
    /// Widzialność (m).
    pub vis: f32,
    pub day: bool,
}

#[derive(Clone, Debug, Default)]
pub struct DayF {
    /// Początek doby (sekundy uniksowe, czas polski liczony przez `sun`).
    pub t: i64,
    pub tmax: f32,
    pub tmin: f32,
    pub precip: f32,
    pub snow: f32,
    pub gust: f32,
    pub code: u8,
}

#[derive(Clone, Debug, Default)]
pub struct Forecast {
    /// Miejsce prognozy (do czasu lokalnego).
    pub lon: f64,
    pub lat: f64,
    pub elevation: f32,
    pub hours: Vec<Hour>,
    pub days: Vec<DayF>,
}

impl Forecast {
    /// Godzina prognozy najbliższa chwili `t`.
    pub fn at(&self, t: i64) -> Option<&Hour> {
        self.hours.iter().min_by_key(|h| (h.t - t).abs()).filter(|h| (h.t - t).abs() <= 3 * 3600)
    }

    /// Godziny od teraz do `hours` naprzód.
    pub fn next(&self, now: i64, hours: usize) -> &[Hour] {
        let i = self.hours.iter().position(|h| h.t + 3600 > now).unwrap_or(self.hours.len());
        &self.hours[i..(i + hours).min(self.hours.len())]
    }
}

#[derive(Clone)]
pub enum State {
    Loading,
    Ready(Arc<Forecast>),
    Failed(String),
}

enum Entry {
    Loading,
    Ready(Arc<Forecast>, Instant),
    Failed(String, Instant),
}

type Key = (i32, i32, i32);

/// Usługa prognoz z pamięcią podręczną; pobiera w tle i odświeża okno, gdy wynik dojdzie.
#[derive(Clone, Default)]
pub struct Weather {
    cache: Arc<Mutex<HashMap<Key, Entry>>>,
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Weather {
    /// Prognoza dla punktu (co ~2 km i 50 m wysokości – sąsiednie zapytania dzielą wynik).
    pub fn get(&self, ctx: &eframe::egui::Context, lon: f64, lat: f64, ele: f32) -> State {
        let key = ((lat * 50.0).round() as i32, (lon * 50.0).round() as i32, (ele / 50.0).round() as i32);
        let mut c = self.cache.lock().unwrap();
        match c.get(&key) {
            Some(Entry::Ready(f, at)) if at.elapsed() < TTL => return State::Ready(f.clone()),
            Some(Entry::Loading) => return State::Loading,
            Some(Entry::Failed(e, at)) if at.elapsed() < RETRY => return State::Failed(e.clone()),
            _ => {}
        }
        // stara prognoza zostaje widoczna, dopóki nie dojdzie nowa
        let stale = match c.get(&key) {
            Some(Entry::Ready(f, _)) => Some(f.clone()),
            _ => None,
        };
        c.insert(key, Entry::Loading);
        drop(c);
        let (cache, ctx) = (self.cache.clone(), ctx.clone());
        let (qlat, qlon, qele) = (key.0 as f64 / 50.0, key.1 as f64 / 50.0, key.2 as f32 * 50.0);
        std::thread::spawn(move || {
            let r = fetch(qlat, qlon, qele);
            let e = match r {
                Ok(f) => Entry::Ready(Arc::new(f), Instant::now()),
                Err(e) => {
                    log::warn!("pogoda: {e}");
                    match stale {
                        Some(f) => Entry::Ready(f, Instant::now() - TTL + RETRY),
                        None => Entry::Failed(e, Instant::now()),
                    }
                }
            };
            cache.lock().unwrap().insert(key, e);
            ctx.request_repaint();
        });
        State::Loading
    }
}

fn fetch(lat: f64, lon: f64, ele: f32) -> Result<Forecast, String> {
    let url = format!(
        "{API}?latitude={lat:.4}&longitude={lon:.4}&elevation={ele:.0}\
         &hourly=temperature_2m,apparent_temperature,wind_speed_10m,wind_gusts_10m,wind_direction_10m,cloud_cover,cloud_cover_low,precipitation,precipitation_probability,snowfall,freezing_level_height,weather_code,visibility,is_day\
         &daily=temperature_2m_max,temperature_2m_min,precipitation_sum,snowfall_sum,wind_gusts_10m_max,weather_code\
         &timezone=auto&timeformat=unixtime&forecast_days=7&wind_speed_unit=kmh"
    );
    let body = crate::download::agent()
        .get(&url)
        .call()
        .map_err(|e| crate::i18n::tr("brak połączenia z serwisem pogody").to_string() + &format!(" ({e})"))?
        .into_string()
        .map_err(|e| e.to_string())?;
    parse(&body)
}

pub fn parse(body: &str) -> Result<Forecast, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    if let Some(r) = v["reason"].as_str() {
        return Err(r.to_string());
    }
    let h = &v["hourly"];
    let col = |k: &str| -> Vec<f32> {
        h[k].as_array().map(|a| a.iter().map(|x| x.as_f64().unwrap_or(f64::NAN) as f32).collect()).unwrap_or_default()
    };
    let times: Vec<i64> = h["time"].as_array().map(|a| a.iter().filter_map(|x| x.as_i64()).collect()).unwrap_or_default();
    let (temp, feels, wind, gust, dir) = (col("temperature_2m"), col("apparent_temperature"), col("wind_speed_10m"), col("wind_gusts_10m"), col("wind_direction_10m"));
    let (cloud, cloud_low, precip, prob, snow) = (col("cloud_cover"), col("cloud_cover_low"), col("precipitation"), col("precipitation_probability"), col("snowfall"));
    let (freezing, code, vis, day) = (col("freezing_level_height"), col("weather_code"), col("visibility"), col("is_day"));
    let g = |a: &[f32], i: usize| a.get(i).copied().filter(|x| x.is_finite()).unwrap_or(0.0);
    let hours = times
        .iter()
        .enumerate()
        .map(|(i, &t)| Hour {
            t,
            temp: g(&temp, i),
            feels: g(&feels, i),
            wind: g(&wind, i),
            gust: g(&gust, i),
            dir: g(&dir, i),
            cloud: g(&cloud, i),
            cloud_low: g(&cloud_low, i),
            precip: g(&precip, i),
            prob: g(&prob, i),
            snow: g(&snow, i),
            freezing: g(&freezing, i),
            code: g(&code, i) as u8,
            vis: vis.get(i).copied().filter(|x| x.is_finite()).unwrap_or(f32::NAN),
            day: g(&day, i) > 0.5,
        })
        .collect::<Vec<_>>();
    let d = &v["daily"];
    let dc = |k: &str| -> Vec<f32> {
        d[k].as_array().map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect()).unwrap_or_default()
    };
    let dt: Vec<i64> = d["time"].as_array().map(|a| a.iter().filter_map(|x| x.as_i64()).collect()).unwrap_or_default();
    let (tmax, tmin, ps, ss, gm, cd) = (dc("temperature_2m_max"), dc("temperature_2m_min"), dc("precipitation_sum"), dc("snowfall_sum"), dc("wind_gusts_10m_max"), dc("weather_code"));
    let days = dt
        .iter()
        .enumerate()
        .map(|(i, &t)| DayF {
            t,
            tmax: g(&tmax, i),
            tmin: g(&tmin, i),
            precip: g(&ps, i),
            snow: g(&ss, i),
            gust: g(&gm, i),
            code: g(&cd, i) as u8,
        })
        .collect();
    if hours.is_empty() {
        return Err(crate::i18n::tr("pusta prognoza").into());
    }
    Ok(Forecast { lon, lat, elevation: v["elevation"].as_f64().unwrap_or(0.0) as f32, hours, days })
}

/// Opis i ikona kodu pogody WMO.
pub fn describe(code: u8, day: bool) -> (&'static str, &'static str) {
    use crate::ui::theme::icon as i;
    match code {
        0 => (if day { i::SUN } else { i::MOON_STARS }, "Bezchmurnie"),
        1 => (if day { i::SUN } else { i::MOON_STARS }, "Prawie bezchmurnie"),
        2 => (if day { i::CLOUD_SUN } else { i::CLOUD_MOON }, "Częściowe zachmurzenie"),
        3 => (i::CLOUD, "Pochmurno"),
        45 | 48 => (i::CLOUD_FOG, "Mgła"),
        51..=57 => (i::CLOUD_RAIN, "Mżawka"),
        61..=67 => (i::CLOUD_RAIN, "Deszcz"),
        71..=77 => (i::CLOUD_SNOW, "Śnieg"),
        80..=82 => (i::CLOUD_RAIN, "Przelotny deszcz"),
        85 | 86 => (i::CLOUD_SNOW, "Przelotny śnieg"),
        95..=99 => (i::CLOUD_LIGHTNING, "Burza"),
        _ => (i::CLOUD, "—"),
    }
}

/// Ostrzeżenie dla godziny w górach (tekst), jeśli warunki są groźne.
pub fn hazards(h: &Hour, ele: f32) -> Vec<&'static str> {
    let mut v = Vec::new();
    if (95..=99).contains(&h.code) {
        v.push("burza");
    }
    if h.gust >= 90.0 {
        v.push("huraganowy wiatr");
    } else if h.gust >= 60.0 {
        v.push("silny wiatr");
    }
    if h.feels <= -20.0 {
        v.push("silny mróz");
    }
    if h.vis.is_finite() && h.vis < 1000.0 {
        v.push("słaba widoczność");
    }
    if h.snow >= 1.0 {
        v.push("opady śniegu");
    } else if h.precip >= 2.0 {
        v.push("intensywny opad");
    }
    if h.temp <= 1.0 && h.temp >= -3.0 && h.precip > 0.1 && h.freezing < ele + 300.0 {
        v.push("możliwe oblodzenie");
    }
    v
}

/// Kierunek wiatru słownie (skąd wieje).
pub fn wind_from(dir: f32) -> &'static str {
    const D: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
    D[(((dir as f64 + 22.5).rem_euclid(360.0)) / 45.0) as usize % 8]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_open_meteo() {
        let body = r#"{"elevation":2499.0,"hourly":{"time":[1762560000,1762563600],"temperature_2m":[-3.5,-4.0],"apparent_temperature":[-9.0,-10.0],"wind_speed_10m":[30,35],"wind_gusts_10m":[55,70],"wind_direction_10m":[270,280],"cloud_cover":[40,90],"cloud_cover_low":[10,80],"precipitation":[0,0.4],"precipitation_probability":[5,60],"snowfall":[0,0.3],"freezing_level_height":[1800,1700],"weather_code":[2,71],"visibility":[24000,null],"is_day":[0,1]},"daily":{"time":[1762556400],"temperature_2m_max":[-1],"temperature_2m_min":[-6],"precipitation_sum":[1.2],"snowfall_sum":[1.0],"wind_gusts_10m_max":[70],"weather_code":[71]}}"#;
        let f = parse(body).unwrap();
        assert_eq!(f.hours.len(), 2);
        assert_eq!(f.hours[1].code, 71);
        assert!(f.hours[1].vis.is_nan());
        assert_eq!(f.days[0].tmin, -6.0);
        assert!(hazards(&f.hours[1], 2499.0).contains(&"silny wiatr"));
        assert_eq!(wind_from(270.0), "W");
        assert_eq!(f.at(1762563000).unwrap().t, 1762563600);
    }
}
