use url::Url;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ResponseUrl(pub Url);

/// Response [`Extensions`][http::Extensions] value that represents intermediate `Url`s traversed by redirects
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct History(pub Vec<Url>);

/// Extension trait for http::response::Builder objects
///
/// Allows the user to add a `Url` to the http::Response
pub trait ResponseBuilderExt {
    /// A builder method for the `http::response::Builder` type that allows the user to add a `Url`
    /// to the `http::Response`
    fn url(self, url: Url) -> Self;

    /// A builder method for the `http::response::Builder` type that allows the user to add redirect history
    /// to the `http::Response`
    fn history(self, history: Vec<Url>) -> Self;
}

impl ResponseBuilderExt for http::response::Builder {
    fn url(self, url: Url) -> Self {
        self.extension(ResponseUrl(url))
    }

    fn history(self, history: Vec<Url>) -> Self {
        self.extension(History(history))
    }
}

#[cfg(test)]
mod tests {
    use super::{History, ResponseBuilderExt, ResponseUrl};
    use http::response::Builder;
    use url::Url;

    #[test]
    fn test_response_builder_ext() {
        let history = vec![
            Url::parse("http://initial.com").unwrap(),
            Url::parse("http://intermediate.com").unwrap(),
        ];
        let url = Url::parse("http://final.com").unwrap();
        let response = Builder::new()
            .status(200)
            .url(url.clone())
            .history(history.clone())
            .body(())
            .unwrap();

        assert_eq!(
            response.extensions().get::<ResponseUrl>(),
            Some(&ResponseUrl(url))
        );

        assert_eq!(
            response.extensions().get::<History>(),
            Some(&History(history))
        );
    }
}
