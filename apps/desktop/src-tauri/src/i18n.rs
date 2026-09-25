//! Translations for the tray menu.
//!
//! The tray is built by the operating system from Rust strings, so it cannot
//! reach the dashboard's dictionaries. It carries its own — a short list,
//! because a tray menu is a short menu — and reads the same `language`
//! setting, so the two surfaces always agree.

use localtrack_core::settings::Language;

/// Everything the tray says.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Key {
    OpenDashboard,
    ClockIn,
    ClockOut,
    TakeBreak,
    Resume,
    PauseTracking,
    ResumeTracking,
    ShowTimer,
    HideTimer,
    Quit,
    NotTracking,
    ClockedOut,
    ClockedIn,
    OnBreak,
    TrackingPaused,
    NoActivity,
    Current,
}

/// Resolve `System` against the desktop's own locale.
///
/// The dashboard asks the webview; there is no webview here, so the
/// environment is the only thing that knows.
pub fn resolve(language: Language) -> Language {
    if language != Language::System {
        return language;
    }
    let tag = std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LC_MESSAGES"))
        .or_else(|_| std::env::var("LANG"))
        .unwrap_or_default()
        .to_ascii_lowercase();
    match tag.split(['_', '.', '-']).next().unwrap_or("") {
        "es" => Language::Es,
        "ca" => Language::Ca,
        "fa" => Language::Fa,
        _ => Language::En,
    }
}

pub fn t(language: Language, key: Key) -> &'static str {
    use Key::*;
    match resolve(language) {
        Language::Es => match key {
            OpenDashboard => "Abrir el panel",
            ClockIn => "Fichar entrada",
            ClockOut => "Fichar salida",
            TakeBreak => "Hacer una pausa",
            Resume => "Reanudar",
            PauseTracking => "Pausar el seguimiento",
            ResumeTracking => "Reanudar el seguimiento",
            ShowTimer => "Mostrar la barra del temporizador",
            HideTimer => "Ocultar la barra del temporizador",
            Quit => "Salir",
            NotTracking => "Sin seguimiento",
            ClockedOut => "Sin fichar",
            ClockedIn => "Fichado",
            OnBreak => "En pausa",
            TrackingPaused => "Seguimiento en pausa",
            NoActivity => "No hay actividad registrada",
            Current => "Ahora",
        },
        Language::Ca => match key {
            OpenDashboard => "Obre el tauler",
            ClockIn => "Fitxa l'entrada",
            ClockOut => "Fitxa la sortida",
            TakeBreak => "Fes una pausa",
            Resume => "Reprèn",
            PauseTracking => "Atura el seguiment",
            ResumeTracking => "Reprèn el seguiment",
            ShowTimer => "Mostra la barra del temporitzador",
            HideTimer => "Amaga la barra del temporitzador",
            Quit => "Surt",
            NotTracking => "Sense seguiment",
            ClockedOut => "Sense fitxar",
            ClockedIn => "Fitxat",
            OnBreak => "En pausa",
            TrackingPaused => "Seguiment aturat",
            NoActivity => "No hi ha activitat registrada",
            Current => "Ara",
        },
        Language::Fa => match key {
            OpenDashboard => "باز کردن داشبورد",
            ClockIn => "ثبت ورود",
            ClockOut => "ثبت خروج",
            TakeBreak => "استراحت",
            Resume => "ادامه",
            PauseTracking => "توقف ردیابی",
            ResumeTracking => "ازسرگیری ردیابی",
            ShowTimer => "نمایش نوار زمان‌سنج",
            HideTimer => "پنهان کردن نوار زمان‌سنج",
            Quit => "خروج از برنامه",
            NotTracking => "در حال ردیابی نیست",
            ClockedOut => "خارج‌شده",
            ClockedIn => "در حال کار",
            OnBreak => "در استراحت",
            TrackingPaused => "ردیابی متوقف است",
            NoActivity => "فعالیتی ثبت نشده",
            Current => "اکنون",
        },
        // English, and the fallback for anything unresolved.
        _ => match key {
            OpenDashboard => "Open dashboard",
            ClockIn => "Clock in",
            ClockOut => "Clock out",
            TakeBreak => "Take a break",
            Resume => "Resume",
            PauseTracking => "Pause tracking",
            ResumeTracking => "Resume tracking",
            ShowTimer => "Show timer window",
            HideTimer => "Hide timer window",
            Quit => "Quit",
            NotTracking => "Not tracking",
            ClockedOut => "Clocked out",
            ClockedIn => "Clocked in",
            OnBreak => "On break",
            TrackingPaused => "Tracking paused",
            NoActivity => "No activity recorded",
            Current => "Current",
        },
    }
}

/// Persian figures, so the tray badge matches the dashboard beside it.
pub fn figures(language: Language, text: &str) -> String {
    if resolve(language) != Language::Fa {
        return text.to_string();
    }
    text.chars()
        .map(|c| match c {
            '0'..='9' => char::from_u32('۰' as u32 + (c as u32 - '0' as u32)).unwrap_or(c),
            other => other,
        })
        .collect()
}
