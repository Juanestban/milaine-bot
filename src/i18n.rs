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
}
