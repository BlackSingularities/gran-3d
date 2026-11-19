//! Polityka prywatności GRANI w programie – ta sama treść co na
//! redzikowski.com/aplikacje/gran/prywatnosc (`src/site/privacy-gran.js` w repozytorium strony).
//! Zmieniaj obie razem; tłumaczenia akapitów są w `lang/*.tsv`.

use eframe::egui::{self, RichText};

use crate::i18n::tr;
use crate::ui::theme;

/// Data wersji polityki (pokazywana nad treścią).
pub const VERSION: &str = "19 listopada 2026";

pub const TITLE: &str = "Polityka prywatności aplikacji GRAŃ";
const LEAD: &str = "GRAŃ to darmowy atlas szlaków 3D na Windows, Androida i iOS. Ta polityka opisuje, jakie dane aplikacja i konto na redzikowski.com przetwarzają, po co, jak długo i jakie masz prawa. Większość danych zostaje na Twoim urządzeniu – na serwer trafia tylko to, co sam wyślesz.";

/// Rozdziały: nagłówek i akapity (po polsku – tłumaczy `tr`).
const SECTIONS: &[(&str, &[&str])] = &[
    (
        "1. Administrator danych",
        &[
            "Administratorem danych jest Adam Rędzikowski, autor aplikacji GRAŃ, prowadzący serwis redzikowski.com. W sprawach danych osobowych napisz na adres obsługi aplikacji podany poniżej albo przez formularz kontaktowy na redzikowski.com.",
        ],
    ),
    (
        "2. Co zostaje na urządzeniu",
        &[
            "Ustawienia, biblioteka tras, miejsc i widoków, pobrane mapy i obszary offline, dziennik programu oraz dane konta (adres e-mail, nazwa i token sesji – bez hasła) są zapisywane tylko na Twoim urządzeniu. Aplikacja nie używa GPS ani lokalizacji urządzenia, nie ma reklam, analityki ani śledzenia.",
            "Położenie GPS (Android, iOS) jest używane tylko po Twojej zgodzie w systemie i tylko wtedy, gdy sam je włączysz – żeby pokazać Cię na mapie i nagrać przejście. Odczyty i nagrane przejścia są zapisywane wyłącznie na urządzeniu; aplikacja nie wysyła położenia na serwer i nikomu go nie udostępnia. Podczas nagrywania na Androidzie widać powiadomienie, a na iOS – wskaźnik lokalizacji na pasku stanu. Plik przejścia trafia do innej aplikacji (np. Garmin Connect, Strava) tylko wtedy, gdy sam go wyeksportujesz albo udostępnisz.",
        ],
    ),
    (
        "3. Konto użytkownika",
        &[
            "Konto jest potrzebne tylko do dodawania punktów, komentarzy, zdjęć i ocen. Przeglądanie mapy i punktów innych osób działa bez konta.",
            "Przy rejestracji zapisujemy: adres e-mail, nazwę widoczną przy Twoich wpisach, skrót hasła (scrypt – samego hasła nie znamy), datę rejestracji i akceptacji tej polityki, datę ostatniego logowania, system i wersję aplikacji oraz skróty tokenów sesji. Na adres e-mail wysyłamy wyłącznie wiadomości o koncie: link aktywacyjny i link do ustawienia nowego hasła – bez newslettera i reklam.",
            "Podstawa prawna: art. 6 ust. 1 lit. b RODO (świadczenie usługi, o którą prosisz, zakładając konto).",
            "Konto, którego nie aktywujesz linkiem z wiadomości, jest usuwane po kilku dniach. Konto możesz w każdej chwili usunąć sam w aplikacji (narzędzie „Punkty i komentarze” → Konto → Usuń konto).",
        ],
    ),
    (
        "4. Punkty, komentarze, zdjęcia i oceny",
        &[
            "Wpisy są publiczne: po zatwierdzeniu przez moderatora zobaczą je wszyscy użytkownicy GRANI razem z Twoją nazwą (nie adresem e-mail). Zapisujemy położenie i wysokość punktu, rodzaj, tytuł, treść, datę, Twoją nazwę i powiązanie z kontem, a do ochrony przed nadużyciami także system, wersję aplikacji, język i skrót identyfikatora instalacji.",
            "Zdjęcia aplikacja przed wysłaniem zmniejsza do najwyżej 2048 pikseli i zapisuje jako JPEG (jakość 75%). Serwer przelicza je ponownie i usuwa wszystkie metadane, w tym położenie GPS, datę i model aparatu. Jedna osoba może dodać najwyżej 3 zdjęcia do punktu. Nie publikuj zdjęć, na których da się rozpoznać inne osoby bez ich zgody, ani danych osobowych w treści wpisów.",
            "Gdy w pobliżu jest już podobny punkt, aplikacja zaproponuje dołączenie wpisu do niego, a moderator może scalić dwa punkty w jeden – komentarze i zdjęcia obu punktów trafiają wtedy do jednego.",
            "Podstawa prawna: art. 6 ust. 1 lit. b RODO (publikacja wpisów, które dodajesz) i art. 6 ust. 1 lit. f RODO (moderacja i bezpieczeństwo serwisu).",
        ],
    ),
    (
        "5. Dane techniczne i bezpieczeństwo",
        &[
            "Serwer nie zapisuje Twojego adresu IP. Do limitów (np. liczby wpisów na kwadrans) i blokowania nadużyć używa jego skrótu z losową solą, którego nie da się zamienić z powrotem na adres. Połączenia są szyfrowane (HTTPS), hasła i tokeny sesji przechowujemy wyłącznie jako skróty.",
            "Podstawa prawna: art. 6 ust. 1 lit. f RODO (uzasadniony interes – bezpieczeństwo serwisu i użytkowników).",
        ],
    ),
    (
        "6. Zgłoszenia błędów i sugestie",
        &[
            "Gdy sam wyślesz zgłoszenie z aplikacji („Zgłoś błąd lub sugestię”) albo zgodzisz się wysłać raport po awarii, trafiają do nas: treść, wersja aplikacji, system, opis urządzenia, przy raporcie o awarii – opis błędu i koniec dziennika programu, a opcjonalnie Twój adres e-mail do odpowiedzi. Nic nie jest wysyłane bez Twojej decyzji. Zgłoszenia usuwamy po ich obsłużeniu.",
        ],
    ),
    (
        "7. Usługi zewnętrzne",
        &[
            "Żeby pokazać mapę, aplikacja pobiera dane bezpośrednio od ich dostawców: kafle i dane OpenStreetMap (OpenFreeMap, serwery Overpass, wyszukiwarka Nominatim), modele terenu (m.in. Mapterhorn, AWS Terrain Tiles), ortofotomapy i mapy krajowe (np. Geoportal, ČÚZK, ZBGIS, swisstopo, Esri), pogodę (Open-Meteo) i komunikaty lawinowe (np. TOPR, laviny.sk, EAWS). Jak przy każdym połączeniu internetowym dostawcy widzą adres IP urządzenia i żądany fragment mapy – nie przekazujemy im żadnych danych konta. Aktualizacje i pliki instalacyjne idą przez redzikowski.com z serwisu GitHub.",
            "Dane konta i wpisów przechowujemy na serwerze redzikowski.com; dostęp do nich ma tylko administrator. Dostawca serwera i poczty e-mail przetwarza je wyłącznie na nasze zlecenie. Nie sprzedajemy danych i nie udostępniamy ich do celów marketingowych.",
        ],
    ),
    (
        "8. Jak długo przechowujemy dane",
        &[
            "Konto i jego dane – do usunięcia konta. Wpisy – dopóki są potrzebne użytkownikom mapy albo do ich usunięcia przez Ciebie lub moderatora. Po usunięciu konta usuwamy Twoje zdjęcia, a punkty i komentarze zostają jako anonimowe (bez nazwy i powiązania z kontem); jeśli chcesz usunąć także je, napisz do nas. Skróty adresów IP do limitów są trzymane w pamięci serwera najwyżej kilkanaście minut.",
        ],
    ),
    (
        "9. Twoje prawa",
        &[
            "Masz prawo dostępu do swoich danych, ich sprostowania, usunięcia, ograniczenia przetwarzania, przeniesienia oraz sprzeciwu wobec przetwarzania opartego na uzasadnionym interesie. Masz też prawo wnieść skargę do Prezesa Urzędu Ochrony Danych Osobowych (ul. Stawki 2, 00-193 Warszawa, uodo.gov.pl). Podanie danych jest dobrowolne, ale bez adresu e-mail i hasła nie założysz konta.",
            "Konto mogą zakładać osoby, które ukończyły 16 lat.",
        ],
    ),
    (
        "10. Zmiany polityki",
        &[
            "O istotnych zmianach poinformujemy w aplikacji. Aktualna wersja jest zawsze w aplikacji i na redzikowski.com/aplikacje/gran/prywatnosc.",
        ],
    ),
];

/// Treść polityki w bieżącym oknie lub panelu.
pub fn body(ui: &mut egui::Ui, support: &str) {
    ui.label(RichText::new(tf!("Wersja z {}", tr(VERSION))).small().color(theme::paper_3()));
    ui.add_space(6.0);
    ui.add(egui::Label::new(RichText::new(tr(LEAD)).color(theme::PAPER)).wrap());
    if !support.is_empty() {
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(tr("Obsługa aplikacji:")).small().color(theme::paper_3()));
            if ui.link(support).clicked() {
                crate::platform::open_url(&format!("mailto:{support}"));
            }
        });
    }
    for (h, ps) in SECTIONS {
        ui.add_space(12.0);
        ui.label(RichText::new(tr(h)).strong().size(15.0));
        for p in *ps {
            ui.add_space(4.0);
            ui.add(egui::Label::new(RichText::new(tr(p)).color(theme::paper_2())).wrap());
        }
    }
    ui.add_space(12.0);
    if ui.link(tr("Polityka na redzikowski.com")).clicked() {
        crate::platform::open_url(crate::account::PRIVACY_URL);
    }
}

/// Okno z polityką prywatności (`open` – czy pokazane).
pub fn window(ctx: &egui::Context, open: &mut bool, support: &str) {
    if !*open {
        return;
    }
    let h = (ctx.screen_rect().height() - 120.0).clamp(240.0, 720.0);
    egui::Window::new(tr(TITLE))
        .id(egui::Id::new("polityka-prywatnosci"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .open(open)
        .show(ctx, |ui| {
            ui.set_width(theme::fit(ctx, 560.0));
            egui::ScrollArea::vertical().max_height(h).show(ui, |ui| body(ui, support));
        });
}
