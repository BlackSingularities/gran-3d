//! Konto użytkownika GRANI na redzikowski.com – potrzebne do dodawania punktów, komentarzy,
//! zdjęć i ocen (przeglądanie mapy działa bez konta).
//!
//! Rejestracja: adres e-mail, hasło i nazwa widoczna przy wpisach; strona wysyła wiadomość
//! z linkiem aktywacyjnym (z poczty aplikacji ustawionej w panelu albo z redzikowski.com).
//! Po aktywacji logowanie daje token sesji, który idzie w nagłówku `Authorization` wpisów.
//! Hasło nie jest zapisywane na urządzeniu.
//!
//! Plik `%APPDATA%\GRAN\konto.json`: adres, nazwa, token, adres obsługi i limity z serwera.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

const API: &str = "https://redzikowski.com/api/apps/gran-3d";
/// Polityka prywatności na stronie (ta sama treść jest w programie: `ui::privacy`).
pub const PRIVACY_URL: &str = "https://redzikowski.com/aplikacje/gran/prywatnosc";
/// Domyślne limity (serwer podaje aktualne w `/info`).
const PHOTOS_PER_USER: u32 = 3;
const MERGE_RADIUS: f64 = 150.0;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Saved {
    #[serde(default)]
    email: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    token: String,
    #[serde(default)]
    user_id: String,
    #[serde(default)]
    support: String,
    #[serde(default)]
    photos_per_user: Option<u32>,
    #[serde(default)]
    photo_scope: String,
    #[serde(default)]
    merge_radius: Option<f64>,
}

/// Co pokazuje okno konta po ostatniej operacji.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Stage {
    #[default]
    Idle,
    /// Konto założone – czeka na kliknięcie linku z wiadomości wysłanej na ten adres.
    Pending(String),
    /// Wysłano link do ustawienia nowego hasła.
    ResetSent(String),
    /// Zalogowano.
    LoggedIn,
    /// Konto usunięte.
    Deleted,
}

#[derive(Default)]
struct State {
    saved: Saved,
    busy: bool,
    error: Option<String>,
    stage: Stage,
    info_asked: bool,
    ctx: Option<eframe::egui::Context>,
}

/// Konto (tanie do klonowania, wspólne dla interfejsu i wysyłki wpisów).
#[derive(Clone, Default)]
pub struct Account {
    inner: Arc<Mutex<State>>,
}

fn file() -> std::path::PathBuf {
    crate::settings::config_dir().join("konto.json")
}

/// Odpowiedź serwera z błędem: kod HTTP, kod błędu (`inactive`, `auth`…) i treść dla użytkownika.
struct Failure {
    status: u16,
    code: String,
    message: String,
}

fn json(r: ureq::Response) -> Result<serde_json::Value, String> {
    let t = r.into_string().map_err(|e| e.to_string())?;
    serde_json::from_str(&t).map_err(|e| e.to_string())
}

fn failure(e: ureq::Error) -> Failure {
    match e {
        ureq::Error::Status(code, r) => {
            let v = json(r).unwrap_or_default();
            let message = v["error"].as_str().map(String::from).unwrap_or_else(|| if code == 429 {
                crate::i18n::tr("Za dużo prób – spróbuj za kilka minut.").to_string()
            } else {
                tf!("serwer odpowiedział kodem {}", code)
            });
            Failure { status: code, code: v["code"].as_str().unwrap_or_default().to_string(), message }
        }
        ureq::Error::Transport(t) => Failure { status: 0, code: String::new(), message: tf!("brak połączenia z redzikowski.com: {}", t) },
    }
}

fn post(path: &str, token: Option<&str>, body: serde_json::Value) -> Result<serde_json::Value, Failure> {
    let mut req = crate::download::agent().post(&format!("{API}{path}")).timeout(Duration::from_secs(25)).set("Content-Type", "application/json");
    if let Some(t) = token {
        req = req.set("Authorization", &format!("Bearer {t}"));
    }
    req.send_string(&body.to_string()).map_err(failure).and_then(|r| json(r).map_err(|message| Failure { status: 0, code: String::new(), message }))
}

fn base(body: &mut serde_json::Value) {
    body["platform"] = crate::platform::NAME.into();
    body["appVersion"] = crate::app_version().into();
    body["locale"] = crate::i18n::current().into();
}

/// Adres e-mail w rozsądnej postaci (serwer sprawdza dokładniej).
pub fn email_ok(s: &str) -> bool {
    let s = s.trim();
    match s.split_once('@') {
        Some((a, b)) => !a.is_empty() && b.contains('.') && !b.starts_with('.') && !b.ends_with('.') && !s.contains(' '),
        None => false,
    }
}

impl Account {
    pub fn load() -> Self {
        let saved: Saved = std::fs::read(file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Self { inner: Arc::new(Mutex::new(State { saved, ..Default::default() })) }
    }

    fn persist(&self) {
        let data = serde_json::to_vec(&self.inner.lock().unwrap().saved).unwrap_or_default();
        let _ = std::fs::create_dir_all(crate::settings::config_dir());
        let _ = crate::pack::write_file(&file(), &data);
    }

    fn repaint(s: &State) {
        if let Some(c) = &s.ctx {
            c.request_repaint();
        }
    }

    pub fn logged_in(&self) -> bool {
        !self.inner.lock().unwrap().saved.token.is_empty()
    }

    /// Nazwa widoczna przy wpisach.
    pub fn name(&self) -> String {
        self.inner.lock().unwrap().saved.name.clone()
    }

    pub fn email(&self) -> String {
        self.inner.lock().unwrap().saved.email.clone()
    }

    pub fn token(&self) -> Option<String> {
        let t = self.inner.lock().unwrap().saved.token.clone();
        (!t.is_empty()).then_some(t)
    }

    pub fn busy(&self) -> bool {
        self.inner.lock().unwrap().busy
    }

    pub fn error(&self) -> Option<String> {
        self.inner.lock().unwrap().error.clone()
    }

    pub fn stage(&self) -> Stage {
        self.inner.lock().unwrap().stage.clone()
    }

    pub fn clear_message(&self) {
        let mut s = self.inner.lock().unwrap();
        s.error = None;
        s.stage = Stage::Idle;
    }

    /// Adres obsługi aplikacji (z serwera; pusty, dopóki nie przyjdzie).
    pub fn support(&self) -> String {
        self.inner.lock().unwrap().saved.support.clone()
    }

    /// Najwięcej zdjęć od jednej osoby (do punktu albo w całej aplikacji – `per_point`).
    pub fn photo_limit(&self) -> (u32, bool) {
        let s = self.inner.lock().unwrap();
        (s.saved.photos_per_user.unwrap_or(PHOTOS_PER_USER), s.saved.photo_scope != "app")
    }

    /// Promień (m), w którym program proponuje dołączenie do istniejącego punktu.
    pub fn merge_radius(&self) -> f64 {
        self.inner.lock().unwrap().saved.merge_radius.unwrap_or(MERGE_RADIUS)
    }

    /// Serwer odrzucił token (wygasł, konto zablokowane albo usunięte w panelu).
    pub fn expire(&self, message: &str) {
        let mut s = self.inner.lock().unwrap();
        if s.saved.token.is_empty() {
            return;
        }
        s.saved.token.clear();
        s.error = Some(message.to_string());
        s.stage = Stage::Idle;
        Self::repaint(&s);
        drop(s);
        self.persist();
    }

    /// Dane aplikacji z serwera (adres obsługi, limity) – raz na uruchomienie, w tle.
    pub fn fetch_info(&self, ctx: &eframe::egui::Context) {
        {
            let mut s = self.inner.lock().unwrap();
            if s.ctx.is_none() {
                s.ctx = Some(ctx.clone());
            }
            if s.info_asked || crate::offline::is_offline() {
                return;
            }
            s.info_asked = true;
        }
        let me = self.clone();
        std::thread::spawn(move || {
            let r = crate::download::agent().get(&format!("{API}/info")).timeout(Duration::from_secs(20)).call().map_err(|e| e.to_string()).and_then(json);
            match r {
                Ok(v) => {
                    {
                        let mut s = me.inner.lock().unwrap();
                        s.saved.support = v["supportEmail"].as_str().unwrap_or_default().to_string();
                        s.saved.photos_per_user = v["limits"]["photosPerUser"].as_u64().map(|n| n as u32);
                        s.saved.photo_scope = v["limits"]["photoScope"].as_str().unwrap_or("point").to_string();
                        s.saved.merge_radius = v["limits"]["mergeRadius"].as_f64();
                    }
                    me.persist();
                }
                Err(e) => {
                    log::info!("konto: dane aplikacji niedostępne: {e}");
                    me.inner.lock().unwrap().info_asked = false;
                }
            }
        });
    }

    /// Operacja w tle: `job` zwraca nowy etap albo błąd; interfejs odświeża się po odpowiedzi.
    fn run(&self, ctx: &eframe::egui::Context, job: impl FnOnce(&Account) -> Result<Stage, Failure> + Send + 'static) {
        {
            let mut s = self.inner.lock().unwrap();
            if s.busy {
                return;
            }
            if crate::offline::is_offline() {
                s.error = Some(crate::i18n::tr("Tryb offline – wyłącz go, żeby połączyć się z kontem.").to_string());
                return;
            }
            s.busy = true;
            s.error = None;
            s.ctx = Some(ctx.clone());
        }
        let me = self.clone();
        std::thread::spawn(move || {
            let r = job(&me);
            let mut s = me.inner.lock().unwrap();
            s.busy = false;
            match r {
                Ok(stage) => s.stage = stage,
                Err(f) => {
                    log::warn!("konto: {} {} {}", f.status, f.code, f.message);
                    if f.code == "inactive" {
                        s.stage = Stage::Pending(s.saved.email.clone());
                    }
                    s.error = Some(f.message);
                }
            }
            Self::repaint(&s);
            drop(s);
            me.persist();
        });
    }

    /// Nowe konto; serwer wysyła link aktywacyjny.
    pub fn register(&self, ctx: &eframe::egui::Context, email: &str, password: &str, name: &str) {
        let (email, password, name) = (email.trim().to_lowercase(), password.to_string(), name.trim().to_string());
        self.run(ctx, move |me| {
            let mut body = serde_json::json!({ "email": email, "password": password, "name": name, "consent": true });
            base(&mut body);
            post("/register", None, body)?;
            let mut s = me.inner.lock().unwrap();
            s.saved.email = email.clone();
            s.saved.name = name;
            Ok(Stage::Pending(email))
        });
    }

    pub fn login(&self, ctx: &eframe::egui::Context, email: &str, password: &str) {
        let (email, password) = (email.trim().to_lowercase(), password.to_string());
        self.run(ctx, move |me| {
            me.inner.lock().unwrap().saved.email = email.clone();
            let mut body = serde_json::json!({ "email": email, "password": password });
            base(&mut body);
            let v = post("/login", None, body)?;
            let mut s = me.inner.lock().unwrap();
            s.saved.token = v["token"].as_str().unwrap_or_default().to_string();
            s.saved.name = v["user"]["name"].as_str().unwrap_or_default().to_string();
            s.saved.user_id = v["user"]["id"].as_str().unwrap_or_default().to_string();
            s.saved.email = v["user"]["email"].as_str().map(String::from).unwrap_or(email);
            Ok(Stage::LoggedIn)
        });
    }

    /// Ponowny link aktywacyjny.
    pub fn resend(&self, ctx: &eframe::egui::Context, email: &str) {
        let email = email.trim().to_lowercase();
        self.run(ctx, move |_| {
            post("/resend", None, serde_json::json!({ "email": email }))?;
            Ok(Stage::Pending(email))
        });
    }

    /// Link do ustawienia nowego hasła.
    pub fn forgot(&self, ctx: &eframe::egui::Context, email: &str) {
        let email = email.trim().to_lowercase();
        self.run(ctx, move |me| {
            post("/forgot", None, serde_json::json!({ "email": email }))?;
            me.inner.lock().unwrap().saved.email = email.clone();
            Ok(Stage::ResetSent(email))
        });
    }

    /// Wylogowanie (token unieważniany na serwerze w tle, lokalnie od razu).
    pub fn logout(&self) {
        let token = {
            let mut s = self.inner.lock().unwrap();
            s.stage = Stage::Idle;
            s.error = None;
            std::mem::take(&mut s.saved.token)
        };
        self.persist();
        if !token.is_empty() && !crate::offline::is_offline() {
            std::thread::spawn(move || {
                let _ = post("/logout", Some(&token), serde_json::json!({}));
            });
        }
    }

    /// Usunięcie konta (po podaniu hasła): zdjęcia znikają, punkty i komentarze zostają anonimowe.
    pub fn delete(&self, ctx: &eframe::egui::Context, password: &str) {
        let Some(token) = self.token() else { return };
        let password = password.to_string();
        self.run(ctx, move |me| {
            post("/me/delete", Some(&token), serde_json::json!({ "password": password }))?;
            let mut s = me.inner.lock().unwrap();
            let email = s.saved.email.clone();
            s.saved = Saved { email, support: s.saved.support.clone(), ..Default::default() };
            Ok(Stage::Deleted)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_shape() {
        assert!(email_ok("ania@example.com"));
        assert!(email_ok(" ania@poczta.pl "));
        assert!(!email_ok("ania"));
        assert!(!email_ok("ania@pl"));
        assert!(!email_ok("a nia@poczta.pl"));
    }

    #[test]
    fn saved_roundtrip() {
        let s = Saved { email: "a@b.pl".into(), name: "Ania".into(), token: "t".into(), photos_per_user: Some(3), ..Default::default() };
        let back: Saved = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.name, "Ania");
        assert_eq!(back.photos_per_user, Some(3));
        let old: Saved = serde_json::from_str("{}").unwrap();
        assert!(old.token.is_empty());
    }
}
