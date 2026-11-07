use std::time::Duration;

/// Languages the bot can reply in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    #[default]
    En,
    Es,
}

impl Lang {
    /// Picks the reply language from a Discord locale such as `es-ES`, `es-419` or `en-US`.
    pub fn from_locale(locale: Option<&str>) -> Self {
        match locale {
            Some(l) if l == "es" || l.starts_with("es-") => Lang::Es,
            _ => Lang::En,
        }
    }
}

/// Every message the bot sends to users.
#[derive(Debug, Clone, Copy)]
pub enum Msg<'a> {
    Pong,
    NotInVoice,
    BusyElsewhere { channel: &'a str },
    UnsupportedLink,
    EmptyQuery,
    ResolveFailed,
    QueueFull { max: usize },
    NowPlayingHeading,
    AddedToQueueHeading,
    QueueLength { len: usize },
}

impl Msg<'_> {
    pub fn render(&self, lang: Lang) -> String {
        use Lang::*;
        match (*self, lang) {
            (Msg::Pong, En) => "Pong! 🎵".into(),
            (Msg::Pong, Es) => "¡Pong! 🎵".into(),
            (Msg::NotInVoice, En) => "Join a voice channel first, then use `/play`.".into(),
            (Msg::NotInVoice, Es) => "Primero entra a un canal de voz y luego usa `/play`.".into(),
            (Msg::BusyElsewhere { channel }, En) => {
                format!("I'm already playing in {channel}. Join me there!")
            }
            (Msg::BusyElsewhere { channel }, Es) => {
                format!("Ya estoy reproduciendo en {channel}. ¡Únete ahí!")
            }
            (Msg::UnsupportedLink, En) => "Only YouTube video links are supported.".into(),
            (Msg::UnsupportedLink, Es) => "Solo se admiten links de videos de YouTube.".into(),
            (Msg::EmptyQuery, En) => "Tell me a song name or a YouTube link.".into(),
            (Msg::EmptyQuery, Es) => "Dime el nombre de una canción o un link de YouTube.".into(),
            (Msg::ResolveFailed, En) => {
                "I couldn't find or load that video. Try another one.".into()
            }
            (Msg::ResolveFailed, Es) => {
                "No pude encontrar o cargar ese video. Prueba con otro.".into()
            }
            (Msg::QueueFull { max }, En) => format!("The queue is full ({max} songs)."),
            (Msg::QueueFull { max }, Es) => format!("La cola está llena ({max} canciones)."),
            (Msg::NowPlayingHeading, En) => "Now Playing 🎶".into(),
            (Msg::NowPlayingHeading, Es) => "Reproduciendo ahora 🎶".into(),
            (Msg::AddedToQueueHeading, En) => "Added to Queue 🎶".into(),
            (Msg::AddedToQueueHeading, Es) => "Agregada a la cola 🎶".into(),
            (Msg::QueueLength { len }, En) => format!("**Queue Length:** {len}"),
            (Msg::QueueLength { len }, Es) => format!("**Canciones en cola:** {len}"),
        }
    }
}

/// `m:ss`, or `h:mm:ss` for tracks longer than an hour.
pub fn fmt_duration(d: Duration) -> String {
    let s = d.as_secs();
    let (h, m, s) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Zero-padded `mm:ss`, or `hh:mm:ss` for tracks longer than an hour (embed titles).
pub fn fmt_clock(d: Duration) -> String {
    let s = d.as_secs();
    let (h, m, s) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 {
        format!("{h:02}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spanish_locales() {
        assert_eq!(Lang::from_locale(Some("es-ES")), Lang::Es);
        assert_eq!(Lang::from_locale(Some("es-419")), Lang::Es);
    }

    #[test]
    fn falls_back_to_english() {
        assert_eq!(Lang::from_locale(Some("en-US")), Lang::En);
        assert_eq!(Lang::from_locale(Some("pt-BR")), Lang::En);
        assert_eq!(Lang::from_locale(Some("estonian")), Lang::En);
        assert_eq!(Lang::from_locale(None), Lang::En);
    }

    #[test]
    fn every_message_differs_per_language() {
        let all = [
            Msg::Pong,
            Msg::NotInVoice,
            Msg::BusyElsewhere { channel: "#music" },
            Msg::UnsupportedLink,
            Msg::EmptyQuery,
            Msg::ResolveFailed,
            Msg::QueueFull { max: 100 },
            Msg::NowPlayingHeading,
            Msg::AddedToQueueHeading,
            Msg::QueueLength { len: 1 },
        ];
        for msg in all {
            let (en, es) = (msg.render(Lang::En), msg.render(Lang::Es));
            assert!(!en.is_empty() && !es.is_empty(), "{msg:?}");
            assert_ne!(en, es, "{msg:?} is not translated");
        }
    }

    #[test]
    fn formats_durations() {
        assert_eq!(fmt_duration(Duration::from_secs(5)), "0:05");
        assert_eq!(fmt_duration(Duration::from_secs(225)), "3:45");
        assert_eq!(fmt_duration(Duration::from_secs(3723)), "1:02:03");
    }

    #[test]
    fn formats_clock() {
        assert_eq!(fmt_clock(Duration::from_secs(233)), "03:53");
        assert_eq!(fmt_clock(Duration::from_secs(5)), "00:05");
        assert_eq!(fmt_clock(Duration::from_secs(3723)), "01:02:03");
    }
}
